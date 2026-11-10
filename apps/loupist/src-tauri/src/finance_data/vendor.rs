//! Native data vendors: the marketplace entries whose agent reads the host
//! makes itself, one row each. A vendor's module owns its requests; this
//! table holds what the rest of the app branches on: the entry id (the
//! capability, credential and pacing key), the hosts its reads reach, what
//! setup stores and verifies, and its read tools. Each tool's turn gates and
//! retention are its `ToolMethod::safety` and `ToolMethod::producer` rows.
//!
//! This is a table, not a trait, because the vendors share no request shape:
//! OpenDART has seven reads behind a company resolver, Korea Investment a
//! token cache, an account profile and a reviewed operation catalog, KRX a
//! service approval per market. A trait would only rename their entry points.
//! Public blockchains (`onchain.wallets`) has no agent tool and reads on the
//! host's own schedule, so it has no row.
use super::{
    DEFILLAMA_SOURCE_ID, KIS_SOURCE_ID, KRX_SOURCE_ID, OPENDART_SOURCE_ID, WORLD_BANK_SOURCE_ID,
};
use crate::broker::ToolMethod;

/// What a vendor's connection stores, and how setup verifies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VendorCredential {
    Keyless,
    /// One `api_key`, verified by a filing list request for today.
    OpenDartKey,
    /// One `api_key`, verified for each KRX daily trading service.
    KrxKey,
    /// An app key and secret, verified by issuing a token in the configured environment.
    KisAppKey,
}

#[derive(Debug)]
pub(crate) struct NativeVendor {
    pub(crate) source_id: &'static str,
    /// Its connector file must declare exactly these `network_hosts`
    /// (checked when the bundled catalog loads).
    pub(crate) hosts: &'static [&'static str],
    pub(crate) credential: VendorCredential,
    /// Its agent read tools. A provider result of one must name `source_id`.
    pub(crate) tools: &'static [ToolMethod],
}

pub(crate) static NATIVE_VENDORS: [NativeVendor; 5] = [
    NativeVendor {
        source_id: OPENDART_SOURCE_ID,
        hosts: &["opendart.fss.or.kr"],
        credential: VendorCredential::OpenDartKey,
        tools: &[
            ToolMethod::DartCompanyLookup,
            ToolMethod::DartFilingSearch,
            ToolMethod::DartFilingRead,
            ToolMethod::DartFinancials,
            ToolMethod::DartMajorReport,
            ToolMethod::DartOwnership,
            ToolMethod::DartDividends,
        ],
    },
    NativeVendor {
        source_id: KRX_SOURCE_ID,
        hosts: &["data-dbg.krx.co.kr"],
        credential: VendorCredential::KrxKey,
        tools: &[ToolMethod::KrxDailyQuote],
    },
    NativeVendor {
        source_id: KIS_SOURCE_ID,
        hosts: &[
            "openapi.koreainvestment.com",
            "openapivts.koreainvestment.com",
        ],
        credential: VendorCredential::KisAppKey,
        tools: &[ToolMethod::KisOperationSearch, ToolMethod::KisRead],
    },
    NativeVendor {
        source_id: WORLD_BANK_SOURCE_ID,
        hosts: &["api.worldbank.org"],
        credential: VendorCredential::Keyless,
        tools: &[ToolMethod::WorldbankSeries],
    },
    NativeVendor {
        source_id: DEFILLAMA_SOURCE_ID,
        hosts: &["api.llama.fi", "stablecoins.llama.fi"],
        credential: VendorCredential::Keyless,
        tools: &[ToolMethod::DefillamaRead],
    },
];

pub(crate) fn native_vendor(source_id: &str) -> Option<&'static NativeVendor> {
    NATIVE_VENDORS
        .iter()
        .find(|vendor| vendor.source_id == source_id)
}

/// The vendor whose read `method` is.
pub(crate) fn serving_vendor(method: ToolMethod) -> Option<&'static NativeVendor> {
    NATIVE_VENDORS
        .iter()
        .find(|vendor| vendor.tools.contains(&method))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::tools::TOOL_CATALOG;

    #[test]
    fn each_vendor_row_matches_its_connector_file() {
        let catalog = crate::marketplace::bundled_catalog().unwrap();
        for vendor in &NATIVE_VENDORS {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.id == vendor.source_id)
                .unwrap_or_else(|| panic!("{} has no connector file", vendor.source_id));
            assert_eq!(
                entry.permissions.network_hosts, vendor.hosts,
                "{}",
                vendor.source_id
            );
            assert_eq!(
                entry.permissions.credential_kinds.is_empty(),
                vendor.credential == VendorCredential::Keyless,
                "{}",
                vendor.source_id
            );
            // Each read tool belongs to the catalog component of this plugin.
            for tool in vendor.tools {
                let spec = TOOL_CATALOG
                    .tools
                    .iter()
                    .find(|spec| spec.name == tool.tool_name())
                    .unwrap();
                let component = TOOL_CATALOG
                    .components
                    .iter()
                    .find(|component| Some(&component.id) == spec.component.as_ref())
                    .unwrap();
                assert_eq!(
                    component.connection.as_deref(),
                    Some(entry.plugin.as_str()),
                    "{tool:?}"
                );
            }
        }
    }

    #[test]
    fn each_api_root_is_a_declared_host_of_its_vendor() {
        let roots = [
            (OPENDART_SOURCE_ID, super::super::OPENDART_API_ROOT),
            (KRX_SOURCE_ID, super::super::KRX_API_ROOT),
            (KIS_SOURCE_ID, super::super::KIS_REAL_API_ROOT),
            (KIS_SOURCE_ID, super::super::KIS_DEMO_API_ROOT),
            (WORLD_BANK_SOURCE_ID, super::super::WORLD_BANK_API_ROOT),
            (
                DEFILLAMA_SOURCE_ID,
                super::super::defillama::DEFILLAMA_API_ROOT,
            ),
            (
                DEFILLAMA_SOURCE_ID,
                super::super::defillama::DEFILLAMA_STABLECOINS_ROOT,
            ),
        ];
        for (source_id, root) in roots {
            let host = reqwest::Url::parse(root).unwrap();
            assert!(
                native_vendor(source_id)
                    .unwrap()
                    .hosts
                    .contains(&host.host_str().unwrap()),
                "{root}"
            );
        }
    }

    #[test]
    fn vendor_tools_are_exactly_the_native_finance_reads() {
        let mut served = Vec::new();
        for vendor in &NATIVE_VENDORS {
            for tool in vendor.tools {
                assert!(!served.contains(tool), "{tool:?} has two vendors");
                served.push(*tool);
            }
        }
        for spec in &TOOL_CATALOG.tools {
            let Some(method) = ToolMethod::from_tool_name(&spec.name) else {
                continue;
            };
            assert_eq!(
                method
                    .producer()
                    .is_some_and(|producer| producer.runtime == "native-finance"),
                served.contains(&method),
                "{method:?}"
            );
        }
    }
}
