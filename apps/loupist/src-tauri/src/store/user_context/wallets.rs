//! Read-only wallet addresses and their latest balance snapshot. The renderer
//! sees masked addresses, network, status and dates; never balances.
use super::wallet_address::{check_address, masked, AddressCheck, WalletFamily};
use super::{insert_snapshot, name, HoldingsPosition, HoldingsSource};
use crate::store::{SqliteStore, StoreError, StoreResult};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const MAX_WALLETS: usize = 10;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletStatus {
    Unread,
    Ok,
    Failed,
}

impl WalletStatus {
    fn parse(value: &str) -> Self {
        match value {
            "ok" => Self::Ok,
            "failed" => Self::Failed,
            _ => Self::Unread,
        }
    }
}

/// What the renderer may know about a wallet. `address` is only for the
/// "Copy address" action; every visible surface uses `masked`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WalletView {
    pub id: String,
    pub family: WalletFamily,
    pub masked: String,
    pub address: String,
    pub name: Option<String>,
    pub added_at_ms: i64,
    pub last_read_at_ms: Option<i64>,
    pub last_good_read_at_ms: Option<i64>,
    pub status: WalletStatus,
    pub unlisted_count: usize,
}

/// The internal identity a host read needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletTarget {
    pub id: String,
    pub family: WalletFamily,
    pub address: String,
}

/// One successful read: positive balances by symbol plus the count of tokens
/// left out because the explorer does not price them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalletRead {
    pub balances: Vec<(String, String)>,
    pub unlisted: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WalletAddError {
    Secret,
    Unsupported,
    Limit,
    Duplicate,
    Store(String),
}

fn view(row: &rusqlite::Row<'_>) -> rusqlite::Result<WalletView> {
    let address: String = row.get(2)?;
    let family: String = row.get(1)?;
    Ok(WalletView {
        id: row.get(0)?,
        family: WalletFamily::parse(&family).unwrap_or(WalletFamily::Bitcoin),
        masked: masked(&address),
        address,
        name: row.get(3)?,
        added_at_ms: row.get(4)?,
        last_read_at_ms: row.get(5)?,
        last_good_read_at_ms: row.get(6)?,
        status: WalletStatus::parse(&row.get::<_, String>(7)?),
        unlisted_count: row.get::<_, i64>(8)?.max(0) as usize,
    })
}

const WALLET_COLUMNS: &str =
    "id,family,address,name,added_at_ms,last_read_at_ms,last_good_read_at_ms,status,unlisted_count";

impl SqliteStore {
    pub fn wallets_list(&self) -> StoreResult<Vec<WalletView>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {WALLET_COLUMNS} FROM user_wallet_addresses ORDER BY added_at_ms, id"
        ))?;
        let wallets = statement.query_map([], view)?.collect::<Result<_, _>>()?;
        Ok(wallets)
    }

    pub fn wallet_targets(&self, wallet_id: Option<&str>) -> StoreResult<Vec<WalletTarget>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id,family,address FROM user_wallet_addresses WHERE ?1 IS NULL OR id = ?1 ORDER BY added_at_ms, id",
        )?;
        let targets = statement
            .query_map([wallet_id], |row| {
                let family: String = row.get(1)?;
                Ok(WalletTarget {
                    id: row.get(0)?,
                    family: WalletFamily::parse(&family).unwrap_or(WalletFamily::Bitcoin),
                    address: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(targets)
    }

    /// Validates in Rust (secret guard first) and stores the canonical
    /// address. A refused secret is never stored, logged or returned.
    pub fn wallet_add(
        &self,
        address: &str,
        wallet_name: Option<&str>,
    ) -> Result<WalletView, WalletAddError> {
        let (family, address) = match check_address(address) {
            AddressCheck::Secret => return Err(WalletAddError::Secret),
            AddressCheck::Unsupported => return Err(WalletAddError::Unsupported),
            AddressCheck::Valid(family, address) => (family, address),
        };
        let wallet_name = name(wallet_name).map_err(|_| {
            WalletAddError::Store("name is too long or contains control characters".into())
        })?;
        let connection = self
            .lock()
            .map_err(|error| WalletAddError::Store(error.to_string()))?;
        let store = |error: rusqlite::Error| WalletAddError::Store(error.to_string());
        let count: i64 = connection
            .query_row("SELECT count(*) FROM user_wallet_addresses", [], |row| {
                row.get(0)
            })
            .map_err(store)?;
        if count as usize >= MAX_WALLETS {
            return Err(WalletAddError::Limit);
        }
        let exists = connection
            .query_row(
                "SELECT 1 FROM user_wallet_addresses WHERE family = ?1 AND address = ?2",
                params![family.as_str(), address],
                |_| Ok(()),
            )
            .optional()
            .map_err(store)?
            .is_some();
        if exists {
            return Err(WalletAddError::Duplicate);
        }
        let id = format!("wallet-{}", uuid::Uuid::new_v4());
        connection
            .execute(
                "INSERT INTO user_wallet_addresses(id,family,address,name,added_at_ms,status) VALUES (?1,?2,?3,?4,?5,'unread')",
                params![id, family.as_str(), address, wallet_name, chrono::Utc::now().timestamp_millis()],
            )
            .map_err(store)?;
        connection
            .query_row(
                &format!("SELECT {WALLET_COLUMNS} FROM user_wallet_addresses WHERE id = ?1"),
                [&id],
                view,
            )
            .map_err(store)
    }

    pub fn wallet_rename(
        &self,
        wallet_id: &str,
        wallet_name: Option<&str>,
    ) -> StoreResult<WalletView> {
        let wallet_name = name(wallet_name)?;
        let connection = self.lock()?;
        if connection.execute(
            "UPDATE user_wallet_addresses SET name = ?2 WHERE id = ?1",
            params![wallet_id, wallet_name],
        )? == 0
        {
            return Err(StoreError::Invalid("wallet is not on this computer".into()));
        }
        Ok(connection.query_row(
            &format!("SELECT {WALLET_COLUMNS} FROM user_wallet_addresses WHERE id = ?1"),
            [wallet_id],
            view,
        )?)
    }

    /// Removes the address and, through the cascade, its balances.
    pub fn wallet_remove(&self, wallet_id: &str) -> StoreResult<()> {
        self.lock()?.execute(
            "DELETE FROM user_wallet_addresses WHERE id = ?1",
            [wallet_id],
        )?;
        Ok(())
    }

    /// A successful read replaces the wallet's snapshot in one transaction.
    pub fn wallet_save_read(&self, wallet_id: &str, read: WalletRead) -> StoreResult<()> {
        let positions = read
            .balances
            .into_iter()
            .filter(|(_, quantity)| quantity != "0")
            .map(|(symbol, quantity)| HoldingsPosition {
                symbol,
                market: super::crypto_file::CRYPTO_MARKET.into(),
                display_name: None,
                quantity,
                cost_basis: None,
                market_value: None,
                currency: String::new(),
            })
            .collect::<Vec<_>>();
        let now = chrono::Utc::now().timestamp_millis();
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let known = tx
            .query_row(
                "SELECT 1 FROM user_wallet_addresses WHERE id = ?1",
                [wallet_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !known {
            // Removed while the read was in flight: keep nothing.
            return Ok(());
        }
        tx.execute(
            "DELETE FROM user_holdings_snapshots WHERE wallet_id = ?1",
            [wallet_id],
        )?;
        insert_snapshot(&tx, HoldingsSource::Wallet, Some(wallet_id), positions, now)?;
        tx.execute(
            "UPDATE user_wallet_addresses SET status='ok', last_read_at_ms=?2, last_good_read_at_ms=?2, unlisted_count=?3 WHERE id=?1",
            params![wallet_id, now, read.unlisted as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A failed read keeps the last good snapshot and records only the attempt.
    pub fn wallet_mark_failed(&self, wallet_id: &str) -> StoreResult<()> {
        self.lock()?.execute(
            "UPDATE user_wallet_addresses SET status='failed', last_read_at_ms=?2 WHERE id=?1",
            params![wallet_id, chrono::Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }
}

impl std::fmt::Display for WalletAddError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Secret => "this looks like a recovery phrase or private key",
            Self::Unsupported => "not a supported address",
            Self::Limit => "you can add up to 10 wallets",
            Self::Duplicate => "this wallet is already added",
            Self::Store(message) => message,
        })
    }
}
