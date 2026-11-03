//! Host-only reads of public wallet balances (the `onchain.wallets` entry).
//! Fixed https hosts, no redirects, bounded responses and one request per
//! second per host. Addresses and balances never reach the agent: there is no
//! tool for this connection and results go straight into the local store.
use super::*;
use crate::store::user_context::{
    decimal_sum, wallet_address::WalletFamily, wallets::WalletRead, wallets::WalletTarget,
};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

pub(crate) const ONCHAIN_SOURCE_ID: &str = "onchain.wallets";
const MEMPOOL_ROOT: &str = "https://mempool.space";
const SOLANA_RPC: &str = "https://api.mainnet-beta.solana.com";
/// One EVM address is read on each of these networks.
pub(crate) const EVM_NETWORKS: [(&str, &str, &str); 5] = [
    ("ethereum", "https://eth.blockscout.com", "ETH"),
    ("base", "https://base.blockscout.com", "ETH"),
    ("arbitrum", "https://arbitrum.blockscout.com", "ETH"),
    ("optimism", "https://optimism.blockscout.com", "ETH"),
    ("polygon", "https://polygon.blockscout.com", "POL"),
];
const MAX_WALLET_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOKEN_ROWS: usize = 1_000;
const HOST_SPACING: Duration = Duration::from_secs(1);

/// Waits so each host sees at most one request per second from this app.
async fn pace(host: &str) {
    static CLOCKS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let wait = {
        let mut clocks = CLOCKS.get_or_init(Default::default).lock().await;
        let now = Instant::now();
        let next = clocks.get(host).copied().unwrap_or(now).max(now);
        clocks.insert(host.to_owned(), next + HOST_SPACING);
        next - now
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

/// An integer amount in base units as an exact decimal string.
pub(crate) fn units(raw: &str, decimals: u32) -> Result<String, FinanceDataError> {
    let raw = raw.trim();
    if raw.is_empty()
        || raw.len() > 80
        || !raw.bytes().all(|byte| byte.is_ascii_digit())
        || decimals > 36
    {
        return Err(FinanceDataError::InvalidResponse);
    }
    let decimals = decimals as usize;
    let padded = format!("{raw:0>width$}", width = decimals + 1);
    let (whole, fraction) = padded.split_at(padded.len() - decimals);
    let text = if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    };
    crate::store::user_context::decimal(&text).map_err(|_| FinanceDataError::InvalidResponse)
}

fn json_integer(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => number.as_u64().map(|value| value.to_string()),
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

/// mempool.space `/api/address/{address}`: confirmed funded minus spent.
pub(crate) fn mempool_balance(bytes: &[u8], address: &str) -> Result<String, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    if !root
        .get("address")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case(address))
    {
        return Err(FinanceDataError::InvalidResponse);
    }
    let stats = root
        .get("chain_stats")
        .ok_or(FinanceDataError::InvalidResponse)?;
    let field = |name: &str| {
        stats
            .get(name)
            .and_then(Value::as_u64)
            .ok_or(FinanceDataError::InvalidResponse)
    };
    let sats = field("funded_txo_sum")?
        .checked_sub(field("spent_txo_sum")?)
        .ok_or(FinanceDataError::InvalidResponse)?;
    units(&sats.to_string(), 8)
}

/// Blockscout `/api/v2/addresses/{address}`: the native coin balance in wei.
pub(crate) fn blockscout_native(bytes: &[u8]) -> Result<String, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let object = root.as_object().ok_or(FinanceDataError::InvalidResponse)?;
    match object.get("coin_balance") {
        None | Some(Value::Null) => Ok("0".into()),
        Some(value) => units(
            &json_integer(value).ok_or(FinanceDataError::InvalidResponse)?,
            18,
        ),
    }
}

/// Blockscout `/api/v2/addresses/{address}/token-balances`: ERC-20 balances the
/// explorer prices. Unpriced tokens (usually airdropped spam) are only counted.
pub(crate) fn blockscout_tokens(
    bytes: &[u8],
) -> Result<(Vec<(String, String)>, usize), FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let rows = root.as_array().ok_or(FinanceDataError::InvalidResponse)?;
    if rows.len() > MAX_TOKEN_ROWS {
        return Err(FinanceDataError::InvalidResponse);
    }
    let (mut listed, mut unlisted) = (Vec::new(), 0);
    for row in rows {
        let token = row.get("token").ok_or(FinanceDataError::InvalidResponse)?;
        if token.get("type").and_then(Value::as_str) != Some("ERC-20") {
            continue;
        }
        let raw = row
            .get("value")
            .and_then(json_integer)
            .ok_or(FinanceDataError::InvalidResponse)?;
        let priced = token.get("exchange_rate").is_some_and(|rate| {
            rate.as_str().is_some_and(|text| !text.trim().is_empty()) || rate.is_number()
        });
        let symbol = token
            .get("symbol")
            .and_then(Value::as_str)
            .map(|symbol| symbol.trim().to_uppercase())
            .filter(|symbol| {
                (1..=16).contains(&symbol.len())
                    && symbol
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'.')
            });
        let decimals = token
            .get("decimals")
            .and_then(json_integer)
            .and_then(|text| text.parse::<u32>().ok())
            .filter(|decimals| *decimals <= 36);
        match (priced, symbol, decimals) {
            (true, Some(symbol), Some(decimals)) => {
                let quantity = units(&raw, decimals)?;
                if quantity != "0" {
                    listed.push((symbol, quantity));
                }
            }
            _ => unlisted += 1,
        }
    }
    Ok((listed, unlisted))
}

/// Solana JSON-RPC `getBalance`: lamports in `result.value`.
pub(crate) fn solana_balance(bytes: &[u8]) -> Result<String, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    if root.get("error").is_some() {
        return Err(FinanceDataError::Provider(
            "Solana RPC refused the balance read".into(),
        ));
    }
    let lamports = root
        .pointer("/result/value")
        .and_then(Value::as_u64)
        .ok_or(FinanceDataError::InvalidResponse)?;
    units(&lamports.to_string(), 9)
}

fn add(
    balances: &mut BTreeMap<String, String>,
    symbol: &str,
    quantity: String,
) -> Result<(), FinanceDataError> {
    let total = match balances.remove(symbol) {
        Some(previous) => {
            decimal_sum(&previous, &quantity).map_err(|_| FinanceDataError::InvalidResponse)?
        }
        None => quantity,
    };
    balances.insert(symbol.to_owned(), total);
    Ok(())
}

impl FinanceDataService {
    /// GET with pacing; a 404 (an address the explorer has never seen) is `None`.
    async fn onchain_get(&self, url: Url) -> Result<Option<Vec<u8>>, FinanceDataError> {
        let host = url
            .host_str()
            .ok_or(FinanceDataError::InvalidResponse)?
            .to_owned();
        pace(&host).await;
        let (status, raw) = self
            .send_provider_bounded(
                self.client.get(url),
                ONCHAIN_SOURCE_ID,
                host,
                MAX_WALLET_RESPONSE_BYTES,
            )
            .await?;
        match status {
            StatusCode::NOT_FOUND => Ok(None),
            StatusCode::TOO_MANY_REQUESTS => Err(FinanceDataError::RateLimited(ONCHAIN_SOURCE_ID)),
            status if !status.is_success() => Err(FinanceDataError::Provider(format!(
                "{ONCHAIN_SOURCE_ID} returned HTTP {}",
                status.as_u16()
            ))),
            _ => Ok(Some(raw.bytes)),
        }
    }

    /// Reads one wallet's public balances. Any failed network fails the whole
    /// read, so the caller keeps the last good snapshot instead of a partial one.
    pub(crate) async fn wallet_read(
        &self,
        target: &WalletTarget,
    ) -> Result<WalletRead, FinanceDataError> {
        let mut balances = BTreeMap::new();
        let mut unlisted = 0;
        match target.family {
            WalletFamily::Bitcoin => {
                let url = fixed_url(MEMPOOL_ROOT, &["api", "address", &target.address])?;
                if let Some(bytes) = self.onchain_get(url).await? {
                    add(
                        &mut balances,
                        "BTC",
                        mempool_balance(&bytes, &target.address)?,
                    )?;
                }
            }
            WalletFamily::Evm => {
                for (_, root, coin) in EVM_NETWORKS {
                    let native = fixed_url(root, &["api", "v2", "addresses", &target.address])?;
                    if let Some(bytes) = self.onchain_get(native).await? {
                        add(&mut balances, coin, blockscout_native(&bytes)?)?;
                    }
                    let tokens = fixed_url(
                        root,
                        &["api", "v2", "addresses", &target.address, "token-balances"],
                    )?;
                    if let Some(bytes) = self.onchain_get(tokens).await? {
                        let (listed, skipped) = blockscout_tokens(&bytes)?;
                        unlisted += skipped;
                        for (symbol, quantity) in listed {
                            add(&mut balances, &symbol, quantity)?;
                        }
                    }
                }
            }
            WalletFamily::Solana => {
                let url = Url::parse(SOLANA_RPC).map_err(|_| FinanceDataError::InvalidResponse)?;
                pace("api.mainnet-beta.solana.com").await;
                let body = json!({"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [target.address]});
                let raw = self
                    .fetch_provider(
                        self.client.post(url).json(&body),
                        ONCHAIN_SOURCE_ID,
                        "api.mainnet-beta.solana.com".into(),
                        MAX_WALLET_RESPONSE_BYTES,
                    )
                    .await?;
                add(&mut balances, "SOL", solana_balance(&raw.bytes)?)?;
            }
        }
        Ok(WalletRead {
            balances: balances
                .into_iter()
                .filter(|(_, quantity)| quantity != "0")
                .collect(),
            unlisted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_units_become_exact_decimals() {
        assert_eq!(units("42710000", 8).unwrap(), "0.4271");
        assert_eq!(units("3200000000000000000", 18).unwrap(), "3.2");
        assert_eq!(units("0", 9).unwrap(), "0");
        assert_eq!(units("1", 18).unwrap(), "0.000000000000000001");
        assert_eq!(
            units("123456789012345678901234567890", 18).unwrap(),
            "123456789012.34567890123456789"
        );
        for bad in ["", "-1", "1.5", "1e3"] {
            assert!(units(bad, 8).is_err(), "{bad}");
        }
    }

    #[test]
    fn mempool_reads_confirmed_funded_minus_spent_for_the_same_address() {
        let address = "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh";
        let body = format!(
            r#"{{"address":"{address}","chain_stats":{{"funded_txo_count":3,"funded_txo_sum":52710000,"spent_txo_count":1,"spent_txo_sum":10000000,"tx_count":4}},"mempool_stats":{{"funded_txo_sum":99900000,"spent_txo_sum":0}}}}"#
        );
        assert_eq!(mempool_balance(body.as_bytes(), address).unwrap(), "0.4271");
        assert!(mempool_balance(body.as_bytes(), "bc1qother").is_err());
        assert!(mempool_balance(br#"{"address":"x"}"#, "x").is_err());
    }

    #[test]
    fn blockscout_keeps_priced_erc20_and_counts_unlisted_tokens() {
        assert_eq!(
            blockscout_native(br#"{"hash":"0x1","coin_balance":"3200000000000000000"}"#).unwrap(),
            "3.2"
        );
        assert_eq!(
            blockscout_native(br#"{"hash":"0x1","coin_balance":null}"#).unwrap(),
            "0"
        );
        let tokens = br#"[
          {"token":{"type":"ERC-20","symbol":"USDC","decimals":"6","exchange_rate":"1.0"},"value":"1250000000"},
          {"token":{"type":"ERC-20","symbol":"VISIT-CLAIM.COM","decimals":"18","exchange_rate":null},"value":"1000000000000000000000"},
          {"token":{"type":"ERC-20","symbol":"AIRDROP","decimals":"18","exchange_rate":null},"value":"5"},
          {"token":{"type":"ERC-721","symbol":"PUNK","decimals":null,"exchange_rate":null},"value":"1"},
          {"token":{"type":"ERC-20","symbol":"ARB","decimals":"18","exchange_rate":"0.31"},"value":"0"}
        ]"#;
        let (listed, unlisted) = blockscout_tokens(tokens).unwrap();
        assert_eq!(listed, vec![("USDC".to_owned(), "1250".to_owned())]);
        assert_eq!(unlisted, 2);
        assert!(blockscout_tokens(br#"{"items":[]}"#).is_err());
    }

    #[test]
    fn solana_reads_lamports_and_refuses_rpc_errors() {
        assert_eq!(
            solana_balance(
                br#"{"jsonrpc":"2.0","result":{"context":{"slot":1},"value":41070000000},"id":1}"#
            )
            .unwrap(),
            "41.07"
        );
        assert!(solana_balance(
            br#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid param"},"id":1}"#
        )
        .is_err());
    }

    #[test]
    fn wallet_hosts_are_the_declared_https_hosts() {
        let manifest: Value = serde_json::from_str(include_str!(
            "../../../marketplace/plugins/onchain/connectors/onchain.wallets.json"
        ))
        .unwrap();
        let declared = manifest["permissions"]["network_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|host| host.as_str().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let used = [MEMPOOL_ROOT, SOLANA_RPC]
            .into_iter()
            .chain(EVM_NETWORKS.iter().map(|(_, root, _)| *root))
            .map(|root| {
                let url = Url::parse(root).unwrap();
                assert_eq!(url.scheme(), "https");
                url.host_str().unwrap().to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(declared, used);
    }
}
