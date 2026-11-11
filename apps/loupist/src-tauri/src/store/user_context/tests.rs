use super::*;
fn store() -> SqliteStore {
    SqliteStore::open_in_memory().unwrap()
}
#[test]
fn watchlist_crud_normalizes_and_deduplicates() {
    let store = store();
    let first = store
        .watchlist_add(" aapl ", " nasdaq ", Some(" Apple "))
        .unwrap();
    assert_eq!(first.symbol, "AAPL");
    assert_eq!(first.market, "NASDAQ");
    assert_eq!(first.display_name.as_deref(), Some("Apple"));
    assert_eq!(store.watchlist_add("AAPL", "NASDAQ", None).unwrap(), first);
    assert_eq!(store.watchlist_list().unwrap().len(), 1);
    store.watchlist_add("aapl", "nyse", None).unwrap();
    assert_eq!(store.watchlist_list().unwrap().len(), 2);
    store.watchlist_remove(" AAPL ", " nasdaq ").unwrap();
    assert_eq!(store.watchlist_list().unwrap().len(), 1);
    assert!(store.watchlist_add("", "NASDAQ", None).is_err());
    assert!(store.watchlist_add("A A", "NASDAQ", None).is_err());
}
#[test]
fn csv_english_preview_commit_preserves_exact_decimals() {
    let store = store();
    let preview = store.preview_csv_bytes(b"ticker,market,name,qty,avg cost,value,currency\r\naapl,NASDAQ,\"Apple, Inc.\",0001.2300,9007199254740993.123456789,120.000,usd\r\n").unwrap();
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.mapping["symbol"], "ticker");
    assert_eq!(preview.positions[0].quantity, "1.23");
    assert_eq!(
        preview.positions[0].cost_basis.as_deref(),
        Some("9007199254740993.123456789")
    );
    assert!(store.holdings_latest().unwrap().is_none());
    let saved = store.holdings_commit_csv(&preview.preview_id).unwrap();
    assert_eq!(saved.currency, "USD");
    assert_eq!(store.holdings_latest().unwrap(), Some(saved.clone()));
    assert!(store.holdings_commit_csv(&preview.preview_id).is_err());
}
#[test]
fn csv_korean_mapping_and_missing_optional_metadata() {
    let preview = store().preview_csv_bytes("\u{feff}종목코드,종목명,보유수량,평균단가,평가금액,통화\n005930,삼성전자,10,65000.125,720000,KRW".as_bytes()).unwrap();
    assert!(preview.errors.is_empty());
    assert_eq!(preview.positions[0].symbol, "005930");
    assert_eq!(preview.positions[0].market, "UNKNOWN");
    assert_eq!(preview.mapping["costBasis"], "평균단가");
    let minimal = store()
        .preview_csv_bytes(b"symbol,quantity\nBRK.B,0.000000001")
        .unwrap();
    assert!(minimal.errors.is_empty());
    assert_eq!(minimal.positions[0].currency, "");
}
#[test]
fn csv_rejects_malformed_duplicates_missing_fields_and_limits() {
    for bytes in [
        b"symbol,quantity\nA,1,extra".as_slice(),
        b"symbol,quantity\nA,NaN",
        b"name,qty\nA,1",
        b"symbol,quantity\nA,1\na,2",
        b"symbol,quantity\n\"A,1",
        b"symbol,ticker,quantity\nA,A,1",
        b"symbol,quantity\nA,1e3",
        b"symbol,quantity",
    ] {
        let store = store();
        let p = store.preview_csv_bytes(bytes).unwrap();
        assert!(!p.errors.is_empty(), "{bytes:?}");
        assert!(store.holdings_commit_csv(&p.preview_id).is_err());
        assert!(store.holdings_latest().unwrap().is_none());
    }
    assert!(store()
        .preview_csv_bytes(&[b'x'; MAX_CSV_BYTES + 1])
        .is_err());
    assert!(store().preview_csv_bytes(&[255]).is_err());
    let csv = format!(
        "symbol,quantity\n{}",
        (0..MAX_HOLDINGS_ROWS)
            .map(|i| format!("S{i},1\n"))
            .collect::<String>()
    );
    assert!(store()
        .preview_csv_bytes(csv.as_bytes())
        .unwrap()
        .errors
        .is_empty());
    let too_many = store()
        .preview_csv_bytes(format!("{csv}EXTRA,1").as_bytes())
        .unwrap();
    assert!(!too_many.errors.is_empty());
    assert!(store()
        .holdings_import_csv(Path::new("/definitely/not/a/file.csv"))
        .is_err());
}
#[test]
fn preview_commit_uses_frozen_bytes_and_cache_is_bounded_and_expires() {
    let store = store();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("holdings.csv");
    std::fs::write(&path, "symbol,quantity\nA,1").unwrap();
    let preview = store.holdings_import_csv(&path).unwrap();
    std::fs::write(&path, "symbol,quantity\nA,999").unwrap();
    assert_eq!(
        store
            .holdings_commit_csv(&preview.preview_id)
            .unwrap()
            .positions[0]
            .quantity,
        "1"
    );
    let mut ids = vec![];
    for _ in 0..8 {
        ids.push(
            store
                .preview_csv_bytes(b"symbol,quantity\nA,1")
                .unwrap()
                .preview_id,
        );
    }
    assert!(store.preview_csv_bytes(b"symbol,quantity\nA,1").is_err());
    store
        .user_context_previews
        .lock()
        .unwrap()
        .get_mut(&ids[0])
        .unwrap()
        .created = Instant::now() - std::time::Duration::from_secs(1801);
    assert!(store.holdings_commit_csv(&ids[0]).is_err());
    assert!(store.preview_csv_bytes(b"symbol,quantity\nA,1").is_ok());
}
#[test]
fn canonical_decimal_never_rounds_or_uses_floats() {
    for (raw, expected) in [
        ("-000.000", "0"),
        ("+.000100", "0.0001"),
        ("-001.2300", "-1.23"),
        (
            "12345678901234567890.1234567890123456789",
            "12345678901234567890.1234567890123456789",
        ),
    ] {
        assert_eq!(decimal(raw).unwrap(), expected);
    }
    for raw in ["NaN", "inf", "1e3", "1,000", "1.2.3", "--1", ".", "+"] {
        assert!(decimal(raw).is_err());
    }
}
#[test]
fn superseded_file_snapshots_and_positions_are_pruned_durably_per_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite3");
    let store = SqliteStore::open_catalog(&path).unwrap();
    stock_csv(&store, b"symbol,quantity\nOLD,1");
    stock_csv(&store, b"symbol,quantity\nNEW,2");
    // An empty successful snapshot clears its source without clearing others.
    store
        .save_holdings_snapshot(HoldingsSource::Kis, vec![])
        .unwrap();
    store
        .save_holdings_snapshot(HoldingsSource::Kis, vec![])
        .unwrap();
    drop(store);
    let store = SqliteStore::open_catalog(&path).unwrap();
    assert_eq!(
        store
            .holdings_latest_positions()
            .unwrap()
            .iter()
            .map(|p| p.symbol.as_str())
            .collect::<Vec<_>>(),
        ["NEW"]
    );
    let db = store.lock().unwrap();
    let snapshots: i64 = db
        .query_row("SELECT count(*) FROM user_holdings_snapshots", [], |r| {
            r.get(0)
        })
        .unwrap();
    let positions: i64 = db
        .query_row("SELECT count(*) FROM user_holdings_positions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!((snapshots, positions), (2, 1));
    drop(db);
    // Refuse a snapshot that the latest ordering would immediately discard.
    let mut db = store.lock().unwrap();
    let tx = db.transaction().unwrap();
    insert_snapshot(&tx, HoldingsSource::CryptoFile, None, vec![], 20).unwrap();
    assert!(matches!(
        insert_snapshot(&tx, HoldingsSource::CryptoFile, None, vec![], 10),
        Err(StoreError::Conflict(_))
    ));
    let latest: i64 = tx
        .query_row(
            "SELECT imported_at_ms FROM user_holdings_snapshots WHERE source='crypto_file'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let count: i64 = tx
        .query_row(
            "SELECT count(*) FROM user_holdings_snapshots WHERE source='crypto_file'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((latest, count), (20, 1));
    tx.commit().unwrap();
}

#[test]
fn wallet_listing_skips_unknown_families_and_statuses() {
    for column in ["family", "status"] {
        let store = store();
        let wallet = store
            .wallet_add("1BoatSLRHtKNngkdXEeobR76b53LETtpyT", None)
            .unwrap();
        store.lock().unwrap().execute("INSERT INTO user_wallet_addresses
            (id,family,address,added_at_ms,status) VALUES ('odd','bitcoin','odd-address',1,'unread')", []).unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                &format!("UPDATE user_wallet_addresses SET {column}='future' WHERE id=?1"),
                ["odd"],
            )
            .unwrap();
        assert_eq!(
            store.wallets_list().unwrap().as_slice(),
            std::slice::from_ref(&wallet),
            "{column}"
        );
        if column == "family" {
            assert!(store.wallet_targets(Some("odd")).is_err());
            assert_eq!(
                store
                    .wallet_targets(None)
                    .unwrap()
                    .iter()
                    .map(|w| w.id.as_str())
                    .collect::<Vec<_>>(),
                [wallet.id.as_str()]
            );
        }
        assert!(store.wallet_rename("odd", None).is_err());
        store.wallet_remove("odd").unwrap();
        assert_eq!(store.wallets_list().unwrap(), [wallet]);
    }
}

#[test]
fn holdings_summary_skips_unknown_sources_and_latest_read_stays_strict() {
    let store = store();
    stock_csv(&store, b"symbol,quantity\nA,1");
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO user_holdings_snapshots(snapshot_id,source,imported_at_ms,currency)
            SELECT 'odd','future',imported_at_ms+1,currency FROM user_holdings_snapshots LIMIT 1",
            [],
        )
        .unwrap();
    assert!(store.holdings_latest().is_err());
    let summary = store.holdings_summary().unwrap();
    assert_eq!(summary.sources.len(), 1);
    assert_eq!(summary.sources[0].source, HoldingsSource::Csv);
    assert_eq!(summary.sources[0].count, 1);
}

#[test]
fn context_is_durable_in_the_app_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("catalog.sqlite3");
    let first = SqliteStore::open_catalog(&path).unwrap();
    first.watchlist_add("aapl", "nasdaq", None).unwrap();
    let preview = first.preview_csv_bytes(b"symbol,quantity\nA,1").unwrap();
    first.holdings_commit_csv(&preview.preview_id).unwrap();
    drop(first);
    let second = SqliteStore::open_catalog(&path).unwrap();
    assert_eq!(second.watchlist_list().unwrap().len(), 1);
    assert!(second.holdings_latest().unwrap().is_some());
}

const KRAKEN_LEDGER: &str = "\"txid\",\"refid\",\"time\",\"type\",\"subtype\",\"aclass\",\"asset\",\"wallet\",\"amount\",\"fee\",\"balance\"\n\
\"L1\",\"R1\",\"2026-10-01 09:00:00\",\"deposit\",\"\",\"currency\",\"ZUSD\",\"spot / main\",\"5000.0000\",\"0.0000\",\"5000.0000\"\n\
\"L2\",\"R2\",\"2026-10-02 10:00:00\",\"trade\",\"\",\"currency\",\"XXBT\",\"spot / main\",\"0.5000000000\",\"0\",\"0.5000000000\"\n\
\"L3\",\"R3\",\"2026-10-20 10:00:00\",\"trade\",\"\",\"currency\",\"XXBT\",\"spot / main\",\"-0.0729000000\",\"0\",\"0.4271000000\"\n\
\"L4\",\"R4\",\"2026-10-03 11:00:00\",\"trade\",\"\",\"currency\",\"XETH\",\"spot / main\",\"2.0000000000\",\"0\",\"2.0000000000\"\n\
\"L5\",\"R5\",\"2026-10-04 11:00:00\",\"staking\",\"\",\"currency\",\"ETH2.S\",\"spot / main\",\"1.2000000000\",\"0\",\"1.2000000000\"\n\
\"L6\",\"R6\",\"2026-10-05 12:00:00\",\"trade\",\"\",\"currency\",\"XXDG\",\"spot / main\",\"-10\",\"0\",\"0.0000000000\"\n\
\"L7\",\"R7\",\"2026-10-31 08:30:00\",\"trade\",\"\",\"currency\",\"SOL\",\"spot / main\",\"41.07\",\"0\",\"41.07\"\n\
\"L8\",\"R8\",\"2026-10-06 12:00:00\",\"deposit\",\"\",\"currency\",\"ZEUR\",\"spot / main\",\"10\",\"0\",\"10\"\n\
\"L9\",\"R9\",\"2026-10-07 12:00:00\",\"deposit\",\"\",\"currency\",\"USDC\",\"spot / main\",\"250\",\"0\",\"250\"\n";

fn stock_csv(store: &SqliteStore, body: &[u8]) {
    let preview = store.preview_csv_bytes(body).unwrap();
    store.holdings_commit_csv(&preview.preview_id).unwrap();
}

#[test]
fn kraken_ledger_keeps_the_latest_balance_and_merges_codes() {
    let store = store();
    let preview = store
        .preview_crypto_bytes(KRAKEN_LEDGER.as_bytes())
        .unwrap();
    assert_eq!(
        preview.format,
        Some(crypto_file::CryptoFileFormat::KrakenLedger)
    );
    assert!(preview.errors.is_empty(), "{:?}", preview.errors);
    assert_eq!(preview.row_count, 9);
    let rows = preview
        .rows
        .iter()
        .map(|row| {
            (
                row.symbol.as_str(),
                row.quantity.as_str(),
                row.from.join(", "),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("BTC", "0.4271", "XXBT".to_owned()),
            ("ETH", "3.2", "ETH2.S, XETH".to_owned()),
            ("SOL", "41.07", "SOL".to_owned()),
            ("USDC", "250", "USDC".to_owned()),
        ]
    );
    let skipped = preview
        .skipped
        .iter()
        .map(|item| (item.symbol.as_str(), item.reason))
        .collect::<Vec<_>>();
    assert_eq!(
        skipped,
        [
            ("DOGE", crypto_file::CryptoSkipReason::Zero),
            ("EUR", crypto_file::CryptoSkipReason::Cash),
            ("USD", crypto_file::CryptoSkipReason::Cash),
        ]
    );
    assert_eq!(
        preview.as_of_ms,
        Some(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 31)
                .unwrap()
                .and_hms_opt(8, 30, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        )
    );
    let saved = store.holdings_commit_csv(&preview.preview_id).unwrap();
    assert_eq!(saved.source, HoldingsSource::CryptoFile);
    assert!(saved.positions.iter().all(|p| p.market == "CRYPTO"));
}

#[test]
fn balance_files_map_unknown_columns_and_refuse_trade_histories() {
    let store = store();
    let korean = store
        .preview_crypto_bytes(
            "코인,보유수량,평가금액\nBTC,0.1,9000000\nKRW,120000,120000\n".as_bytes(),
        )
        .unwrap();
    assert_eq!(korean.format, Some(crypto_file::CryptoFileFormat::Balances));
    assert_eq!(korean.rows.len(), 1);
    assert_eq!(korean.skipped[0].symbol, "KRW");

    let unknown = store
        .preview_crypto_bytes(b"Token,Holding,Note\nBTC,0.25,cold\nETH,1.5,\n")
        .unwrap();
    assert_eq!(
        unknown.mapping_headers.as_deref(),
        Some(&["Token".to_owned(), "Holding".to_owned(), "Note".to_owned()][..])
    );
    assert!(store.holdings_commit_csv(&unknown.preview_id).is_err());
    assert!(store
        .holdings_map_columns(&unknown.preview_id, "Token", "Token")
        .is_err());
    assert!(store
        .holdings_map_columns(&unknown.preview_id, "Token", "Missing")
        .is_err());
    let mapped = store
        .holdings_map_columns(&unknown.preview_id, "Token", "Holding")
        .unwrap();
    assert_eq!(mapped.format, Some(crypto_file::CryptoFileFormat::Mapped));
    assert_eq!(
        mapped
            .rows
            .iter()
            .map(|row| row.symbol.as_str())
            .collect::<Vec<_>>(),
        ["BTC", "ETH"]
    );
    assert_eq!(
        store
            .holdings_commit_csv(&unknown.preview_id)
            .unwrap()
            .positions
            .len(),
        2
    );

    for trades in [
        b"Date,Pair,Side,Price,Amount\n2026-10-01,BTC/USD,buy,60000,0.1\n".as_slice(),
        b"time,type,asset,amount\n2026-10-01,buy,BTC,0.1\n",
    ] {
        let preview = store.preview_crypto_bytes(trades).unwrap();
        assert!(preview.trades_only, "{:?}", String::from_utf8_lossy(trades));
        assert!(preview.rows.is_empty() && preview.mapping_headers.is_none());
        assert!(store.holdings_commit_csv(&preview.preview_id).is_err());
        assert!(store
            .holdings_map_columns(&preview.preview_id, "asset", "amount")
            .is_err());
    }
    // Negative or malformed balances are listed by row and block the import.
    let bad = store
        .preview_crypto_bytes(b"asset,balance\nBTC,-1\nETH,1e3\n")
        .unwrap();
    assert_eq!(
        bad.errors.iter().map(|error| error.row).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(store.holdings_commit_csv(&bad.preview_id).is_err());
}

#[test]
fn holdings_are_the_union_of_the_latest_snapshot_per_source() {
    let store = store();
    stock_csv(
        &store,
        b"symbol,market,quantity\nAAPL,NASDAQ,10\nMSFT,NASDAQ,3\n",
    );
    let crypto = store
        .preview_crypto_bytes(KRAKEN_LEDGER.as_bytes())
        .unwrap();
    store.holdings_commit_csv(&crypto.preview_id).unwrap();
    let held = |store: &SqliteStore| {
        store
            .map_context()
            .unwrap()
            .holdings
            .into_iter()
            .map(|item| format!("{}:{}", item.symbol, item.market))
            .collect::<Vec<_>>()
    };
    // A crypto import never replaces stock holdings.
    assert_eq!(
        held(&store),
        [
            "AAPL:NASDAQ",
            "BTC:CRYPTO",
            "ETH:CRYPTO",
            "MSFT:NASDAQ",
            "SOL:CRYPTO",
            "USDC:CRYPTO"
        ]
    );
    // A newer stock import replaces only the stock file's snapshot.
    stock_csv(&store, b"symbol,market,quantity\nAAPL,NASDAQ,11\n");
    assert_eq!(
        held(&store),
        [
            "AAPL:NASDAQ",
            "BTC:CRYPTO",
            "ETH:CRYPTO",
            "SOL:CRYPTO",
            "USDC:CRYPTO"
        ]
    );
    // A wallet holding the same symbol is still one held item.
    let wallet = store
        .wallet_add(
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3",
            Some("Cold storage"),
        )
        .unwrap();
    store
        .wallet_save_read(
            &wallet.id,
            wallets::WalletRead {
                balances: vec![("BTC".into(), "0.9".into())],
                unlisted: 0,
            },
        )
        .unwrap();
    assert_eq!(
        held(&store)
            .iter()
            .filter(|item| item.starts_with("BTC"))
            .count(),
        1
    );

    let summary = store.holdings_summary().unwrap();
    assert_eq!(
        summary
            .sources
            .iter()
            .map(|s| (s.source, s.count))
            .collect::<Vec<_>>(),
        [(HoldingsSource::Csv, 1), (HoldingsSource::CryptoFile, 4)]
    );
    assert_eq!(summary.stocks.as_ref().unwrap().count, 1);
    assert_eq!(summary.crypto.as_ref().unwrap().count, 4);
    // Counts and dates only: no quantity crosses this read.
    let json = serde_json::to_string(&summary).unwrap();
    for canary in ["0.4271", "41.07", "0.9", "\"11\""] {
        assert!(!json.contains(canary), "{canary} in {json}");
    }
}

#[test]
fn wallets_validate_refuse_secrets_and_keep_the_last_good_read() {
    let store = store();
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    for (input, expected) in [
        (phrase, wallets::WalletAddError::Secret),
        (
            "0c28fca386c7a227600b2fe50b7cae11ec86d3bf1fbe471be89827e19d72aa1d",
            wallets::WalletAddError::Secret,
        ),
        (
            "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv4",
            wallets::WalletAddError::Unsupported,
        ),
    ] {
        let error = store.wallet_add(input, None).unwrap_err();
        assert!(!error.to_string().contains(input) && !error.to_string().contains("abandon"));
        assert_eq!(error, expected);
    }
    // Nothing about a refused secret was stored anywhere in the app catalog.
    let rows: i64 = store
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM user_wallet_addresses", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);

    let evm = store
        .wallet_add("0xfb6916095ca1df60bb79ce92ce3ea74c37c5d359", None)
        .unwrap();
    assert_eq!(evm.family, wallet_address::WalletFamily::Evm);
    assert_eq!(evm.masked, "0xfB69…d359");
    assert_eq!(evm.status, wallets::WalletStatus::Unread);
    assert_eq!(
        store
            .wallet_add("0xFB6916095CA1DF60BB79CE92CE3EA74C37C5D359", None)
            .unwrap_err(),
        wallets::WalletAddError::Duplicate
    );
    store
        .wallet_save_read(
            &evm.id,
            wallets::WalletRead {
                balances: vec![
                    ("ETH".into(), "3.14159265".into()),
                    ("USDC".into(), "0".into()),
                ],
                unlisted: 7,
            },
        )
        .unwrap();
    let read = store.wallets_list().unwrap().remove(0);
    assert_eq!(
        (read.status, read.unlisted_count),
        (wallets::WalletStatus::Ok, 7)
    );
    let good_at = read.last_good_read_at_ms;
    store.wallet_mark_failed(&evm.id).unwrap();
    let failed = store.wallets_list().unwrap().remove(0);
    assert_eq!(failed.status, wallets::WalletStatus::Failed);
    assert_eq!(failed.last_good_read_at_ms, good_at);
    // The failed read kept the last good snapshot.
    assert_eq!(
        store
            .map_context()
            .unwrap()
            .holdings
            .iter()
            .map(|item| item.symbol.as_str())
            .collect::<Vec<_>>(),
        ["ETH"]
    );
    // The list carries no balance.
    let json = serde_json::to_string(&store.wallets_list().unwrap()).unwrap();
    assert!(!json.contains("3.14159265"));

    store.wallet_rename(&evm.id, Some("Hot wallet")).unwrap();
    assert_eq!(
        store.wallets_list().unwrap()[0].name.as_deref(),
        Some("Hot wallet")
    );
    store.wallet_remove(&evm.id).unwrap();
    assert!(store.map_context().unwrap().holdings.is_empty());
    assert!(store.wallets_list().unwrap().is_empty());

    for index in 0..wallets::MAX_WALLETS {
        store
            .wallet_add(&format!("0x{:040x}", index + 1), None)
            .unwrap();
    }
    assert_eq!(
        store
            .wallet_add("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", None)
            .unwrap_err(),
        wallets::WalletAddError::Limit
    );
}

#[test]
fn decimal_sum_is_exact() {
    for (left, right, sum) in [
        ("0.5", "0.5", "1"),
        ("2", "1.2", "3.2"),
        ("99.99", "0.01", "100"),
        ("0.000000000000000001", "1", "1.000000000000000001"),
        ("0", "0", "0"),
    ] {
        assert_eq!(decimal_sum(left, right).unwrap(), sum);
    }
    assert!(decimal_sum("-1", "1").is_err());
}
