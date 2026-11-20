//! Catalog-only access to retired ownership rows. No renderer or agent reads these rows.
use rusqlite::{types::ValueRef, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{put_app_setting, SqliteStore, StoreError, StoreResult};

const RECEIPT_ID: &str = "legacy_holdings_export";
const TABLES: [&str; 3] = [
    "retired_wallet_addresses",
    "retired_holdings_snapshots",
    "retired_holdings_positions",
];
// Frozen store-7 columns, including nullable fields and decimal strings.
const COLUMNS: [&str; 3] = [
    "id,family,address,name,added_at_ms,last_read_at_ms,last_good_read_at_ms,status,unlisted_count",
    "snapshot_id,source,wallet_id,imported_at_ms,currency,note",
    "snapshot_id,symbol,market,display_name,quantity,cost_basis,market_value,currency",
];
const ORDER: [&str; 3] = ["id", "snapshot_id", "snapshot_id,symbol,market"];

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LegacyHoldingsCounts {
    pub wallets: usize,
    pub snapshots: usize,
    pub positions: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Receipt {
    pub state: String,
    pub file_name: String,
    pub sha256: String,
    pub counts: LegacyHoldingsCounts,
    pub exported_at_ms: i64,
    pub dismissed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Rows {
    pub tables_exist: bool,
    pub wallets: Vec<Value>,
    pub snapshots: Vec<Value>,
    pub positions: Vec<Value>,
}

impl Rows {
    pub fn counts(&self) -> LegacyHoldingsCounts {
        LegacyHoldingsCounts {
            wallets: self.wallets.len(),
            snapshots: self.snapshots.len(),
            positions: self.positions.len(),
        }
    }
}

fn exists(connection: &Connection, table: &str) -> StoreResult<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn validate_columns(connection: &Connection, index: usize) -> StoreResult<()> {
    let mut statement =
        connection.prepare("SELECT name FROM pragma_table_xinfo(?1) ORDER BY cid")?;
    let columns = statement
        .query_map([TABLES[index]], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns
        .iter()
        .map(String::as_str)
        .ne(COLUMNS[index].split(','))
    {
        return Err(StoreError::Invalid(
            "retired holdings columns do not match store 7".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadKind {
    Receipt,
    Rows,
}
#[cfg(test)]
thread_local! {
    static READ_PANIC: std::cell::Cell<Option<(ReadKind, usize)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) fn with_read_panic<T>(kind: ReadKind, skip: usize, work: impl FnOnce() -> T) -> T {
    struct Reset(Option<(ReadKind, usize)>);
    impl Drop for Reset {
        fn drop(&mut self) {
            READ_PANIC.set(self.0);
        }
    }
    let _reset = Reset(READ_PANIC.replace(Some((kind, skip))));
    work()
}
#[cfg(test)]
fn before_read(kind: ReadKind) {
    if let Some((target, skip)) = READ_PANIC.get() {
        if kind == target {
            if skip == 0 {
                // Consume the hook before unwinding, so recovery status can read.
                READ_PANIC.set(None);
                panic!("test legacy holdings read panic");
            }
            READ_PANIC.set(Some((target, skip - 1)));
        }
    }
}

fn read_rows(connection: &Connection) -> StoreResult<Rows> {
    #[cfg(test)]
    before_read(ReadKind::Rows);
    let mut result = Rows::default();
    for (index, target) in [
        &mut result.wallets,
        &mut result.snapshots,
        &mut result.positions,
    ]
    .into_iter()
    .enumerate()
    {
        if !exists(connection, TABLES[index])? {
            continue;
        }
        validate_columns(connection, index)?;
        result.tables_exist = true;
        let mut statement = connection.prepare(&format!(
            "SELECT {} FROM {} ORDER BY {}",
            COLUMNS[index], TABLES[index], ORDER[index]
        ))?;
        let names: Vec<_> = COLUMNS[index].split(',').collect();
        let rows = statement.query_map([], |row| {
            let mut object = Map::new();
            for (column, name) in names.iter().enumerate() {
                let value = match row.get_ref(column)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(value) => Value::from(value),
                    ValueRef::Text(value) => Value::String(
                        std::str::from_utf8(value)
                            .map_err(|e| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    column,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                )
                            })?
                            .into(),
                    ),
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                object.insert((*name).into(), value);
            }
            Ok(Value::Object(object))
        })?;
        *target = rows.collect::<Result<_, _>>()?;
    }
    Ok(result)
}

impl SqliteStore {
    pub(crate) fn legacy_holdings_receipt(&self) -> StoreResult<Option<Receipt>> {
        let connection = self.lock()?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(test)]
            before_read(ReadKind::Receipt);
            let json: Option<String> = connection
                .query_row(
                    "SELECT data_json FROM app_settings WHERE id=?1",
                    [RECEIPT_ID],
                    |row| row.get(0),
                )
                .optional()?;
            json.map(|value| serde_json::from_str(&value).map_err(Into::into))
                .transpose()
        }));
        drop(connection);
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    pub(crate) fn legacy_holdings_rows(&self) -> StoreResult<Rows> {
        let mut connection = self.lock()?;
        // The transaction unwinds inside this boundary; release its connection
        // guard normally before propagating the panic to the export run guard.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let transaction = connection.transaction()?;
            let rows = read_rows(&transaction)?;
            transaction.commit()?;
            Ok(rows)
        }));
        drop(connection);
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    pub(crate) fn legacy_holdings_tables_exist(&self) -> StoreResult<bool> {
        let connection = self.lock()?;
        for table in TABLES {
            if exists(&connection, table)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Receipt and deletion share one transaction; recheck the exact rows before deleting.
    pub(crate) fn legacy_holdings_commit(
        &self,
        expected: &Rows,
        receipt: &Receipt,
        before_commit: impl FnOnce(&rusqlite::Transaction<'_>) -> StoreResult<()>,
    ) -> StoreResult<()> {
        let mut connection = self.lock()?;
        // Roll a panic back inside the connection's lock scope, then resume it
        // after dropping the lock normally so the next export can use SQLite.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if read_rows(&transaction)? != *expected {
                return Err(StoreError::Conflict(
                    "retired holdings changed during export",
                ));
            }
            put_app_setting(&transaction, RECEIPT_ID, &serde_json::to_string(receipt)?)?;
            // This is the only deletion site for retired tables, after a verified export.
            transaction.execute_batch(
                "DROP TABLE IF EXISTS retired_holdings_positions;
             DROP TABLE IF EXISTS retired_holdings_snapshots;
             DROP TABLE IF EXISTS retired_wallet_addresses;",
            )?;
            before_commit(&transaction)?;
            transaction.commit()?;
            Ok(())
        }));
        drop(connection);
        match result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    pub(crate) fn legacy_holdings_dismiss(&self) -> StoreResult<()> {
        if let Some(mut receipt) = self.legacy_holdings_receipt()? {
            receipt.dismissed = true;
            let connection = self.lock()?;
            put_app_setting(&connection, RECEIPT_ID, &serde_json::to_string(&receipt)?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    /// Build the real ladder's input, rather than manufacturing retired tables.
    pub(crate) fn fixture(root: &Path, version7: bool) -> SqliteStore {
        let path = root.join("catalog.sqlite3");
        let mut connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!(
                "tests/fixtures/baseline-store4-knowledge6.sql"
            ))
            .unwrap();
        connection.execute_batch(
            "INSERT INTO user_wallet_addresses VALUES ('w1','evm','0xabc','Main',1,2,3,'ok',4);
             INSERT INTO user_holdings_snapshots VALUES ('s1','wallet','w1',5,'USD',NULL);
             INSERT INTO user_holdings_snapshots VALUES ('s2','csv',NULL,6,'KRW','broker');
             INSERT INTO user_holdings_positions VALUES ('s1','ETH','CRYPTO','Ether','2.0000',NULL,'123.45','USD');"
        ).unwrap();
        if version7 {
            // Store 5 removes enum checks; 6 and 7 add indexes/follow seeding.
            super::super::upgrade::upgrade_store_for_test(&mut connection, 7);
            connection.execute_batch(
                "INSERT INTO user_wallet_addresses VALUES ('w2','solana','unread',NULL,8,NULL,NULL,'unread',0);
                 INSERT INTO user_wallet_addresses VALUES ('w3','evm','0xdef','Other',9,10,NULL,'failed',2);
                 INSERT INTO user_holdings_snapshots VALUES ('s3','kis',NULL,10,'EUR',NULL);
                 INSERT INTO user_holdings_snapshots VALUES ('s4','crypto_file',NULL,11,'USD','exchange');
                 INSERT INTO user_holdings_positions VALUES ('s2','ABC','KRX',NULL,'0.00001','001.20',NULL,'KRW');
                 INSERT INTO user_holdings_positions VALUES ('s1','BTC','CRYPTO',NULL,'3',NULL,NULL,'USD');"
            ).unwrap();
        }
        drop(connection);
        SqliteStore::open(&path).unwrap()
    }

    #[test]
    fn missing_tables_are_empty_and_receipt_deletion_is_atomic() {
        let root = tempfile::tempdir().unwrap();
        let store = fixture(root.path(), false);
        let rows = store.legacy_holdings_rows().unwrap();
        assert_eq!(
            rows.counts(),
            LegacyHoldingsCounts {
                wallets: 1,
                snapshots: 2,
                positions: 1
            }
        );
        let receipt = Receipt {
            state: "exported".into(),
            file_name: "holdings-export.json".into(),
            sha256: "a".repeat(64),
            counts: rows.counts(),
            exported_at_ms: 1,
            dismissed: false,
        };
        assert!(store
            .legacy_holdings_commit(&rows, &receipt, |tx| {
                assert!(tx.query_row("SELECT EXISTS(SELECT 1 FROM app_settings WHERE id='legacy_holdings_export')", [], |r| r.get::<_, bool>(0)).unwrap());
                assert!(!exists(tx, "retired_wallet_addresses").unwrap());
                let observer = Connection::open(root.path().join("catalog.sqlite3")).unwrap();
                assert!(exists(&observer, "retired_wallet_addresses").unwrap());
                assert!(!observer.query_row("SELECT EXISTS(SELECT 1 FROM app_settings WHERE id='legacy_holdings_export')", [], |r| r.get::<_, bool>(0)).unwrap());
                Err(StoreError::Invalid("rollback".into()))
            })
            .is_err());
        assert_eq!(store.legacy_holdings_rows().unwrap(), rows);
        assert!(store.legacy_holdings_receipt().unwrap().is_none());
        store
            .legacy_holdings_commit(&rows, &receipt, |_| Ok(()))
            .unwrap();
        assert!(!store.legacy_holdings_rows().unwrap().tables_exist);
        assert_eq!(
            store.legacy_holdings_receipt().unwrap().unwrap().counts,
            rows.counts()
        );
    }
    #[test]
    fn missing_one_table_is_empty_and_changed_rows_refuse_the_receipt() {
        let root = tempfile::tempdir().unwrap();
        let store = fixture(root.path(), false);
        // Model a catalog that retained only two of the retired tables.
        store
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE retired_holdings_positions")
            .unwrap();
        let rows = store.legacy_holdings_rows().unwrap();
        assert_eq!(
            rows.counts(),
            LegacyHoldingsCounts {
                wallets: 1,
                snapshots: 2,
                positions: 0
            }
        );
        store
            .lock()
            .unwrap()
            .execute("UPDATE retired_wallet_addresses SET name='new value'", [])
            .unwrap();
        let receipt = Receipt {
            state: "exported".into(),
            file_name: "holdings-export.json".into(),
            sha256: "a".repeat(64),
            counts: rows.counts(),
            exported_at_ms: 1,
            dismissed: false,
        };
        assert!(store
            .legacy_holdings_commit(&rows, &receipt, |_| Ok(()))
            .is_err());
        assert!(store.legacy_holdings_tables_exist().unwrap());
        assert!(store.legacy_holdings_receipt().unwrap().is_none());
    }
}
