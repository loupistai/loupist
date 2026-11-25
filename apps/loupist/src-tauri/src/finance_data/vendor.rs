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
use super::{
    openfigi::SOURCE_ID as ASSET_LOOKUP_SOURCE_ID, DEFILLAMA_SOURCE_ID, KIS_SOURCE_ID,
    KRX_SOURCE_ID, OPENDART_SOURCE_ID, WORLD_BANK_SOURCE_ID,
};
use crate::broker::ToolMethod;

/// What a vendor's connection stores, and how setup verifies it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VendorCredential {
    Keyless,
    /// An optional personal key used directly by authorized listing lookups.
    OptionalOpenFigiKey,
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
    /// Query parameters that carry its credential on a request URL. They are
    /// removed from every locator (see `public_locator`) and refused in results.
    pub(crate) credential_query: &'static [&'static str],
    /// Its agent read tools. A provider result of one must name `source_id`.
    pub(crate) tools: &'static [ToolMethod],
}

pub(crate) static NATIVE_VENDORS: [NativeVendor; 6] = [
    NativeVendor {
        source_id: ASSET_LOOKUP_SOURCE_ID,
        hosts: &["api.openfigi.com"],
        credential: VendorCredential::OptionalOpenFigiKey,
        credential_query: &[],
        tools: &[],
    },
    NativeVendor {
        source_id: OPENDART_SOURCE_ID,
        hosts: &["opendart.fss.or.kr"],
        credential: VendorCredential::OpenDartKey,
        credential_query: &["crtfc_key"],
        tools: &[
            ToolMethod::DartCompanyLookup,
            ToolMethod::DartFilingSearch,
            ToolMethod::DartFilingRead,
            ToolMethod::DartFinancials,
            ToolMethod::DartMajorReport,
            ToolMethod::DartOwnership,
            ToolMethod::DartDividends,
            ToolMethod::DartReportItems,
        ],
    },
    NativeVendor {
        source_id: KRX_SOURCE_ID,
        hosts: &["data-dbg.krx.co.kr"],
        credential: VendorCredential::KrxKey,
        credential_query: &[],
        tools: &[ToolMethod::KrxDailyQuote],
    },
    NativeVendor {
        source_id: KIS_SOURCE_ID,
        hosts: &[
            "openapi.koreainvestment.com",
            "openapivts.koreainvestment.com",
            "new.real.download.dws.co.kr",
        ],
        credential: VendorCredential::KisAppKey,
        credential_query: &[],
        tools: &[
            ToolMethod::KisOperationSearch,
            ToolMethod::KisSymbolLookup,
            ToolMethod::KisDividendCalendar,
            ToolMethod::KisRead,
        ],
    },
    NativeVendor {
        source_id: WORLD_BANK_SOURCE_ID,
        hosts: &["api.worldbank.org"],
        credential: VendorCredential::Keyless,
        credential_query: &[],
        tools: &[ToolMethod::WorldbankSeries],
    },
    NativeVendor {
        source_id: DEFILLAMA_SOURCE_ID,
        hosts: &["api.llama.fi", "stablecoins.llama.fi"],
        credential: VendorCredential::Keyless,
        credential_query: &[],
        tools: &[ToolMethod::DefillamaRead],
    },
];

pub(crate) fn native_vendor(source_id: &str) -> Option<&'static NativeVendor> {
    NATIVE_VENDORS
        .iter()
        .find(|vendor| vendor.source_id == source_id)
}

/// Every query parameter a native vendor declares as its credential.
pub(crate) fn credential_query_parameters() -> impl Iterator<Item = &'static str> {
    NATIVE_VENDORS
        .iter()
        .flat_map(|vendor| vendor.credential_query.iter().copied())
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

    /// No locator (a result's source_url, so its Source, citation and export)
    /// and no log line may carry a parameter a vendor declares as its credential.
    #[test]
    fn no_locator_or_log_line_keeps_a_declared_credential_parameter() {
        assert!(super::credential_query_parameters().any(|parameter| parameter == "crtfc_key"));
        for vendor in &NATIVE_VENDORS {
            for parameter in vendor.credential_query {
                for name in [parameter.to_string(), parameter.to_ascii_uppercase()] {
                    let url = format!(
                        "https://{}/api/corpCode.xml?corp_code=00126380&{name}=fixture-secret-0000&page_no=2",
                        vendor.hosts[0]
                    );
                    assert!(super::super::locator_carries_credential(&url), "{url}");
                    let locator = super::super::public_locator(&url);
                    assert!(!locator.contains("fixture-secret"), "{locator}");
                    assert!(!super::super::locator_carries_credential(&locator));
                    assert!(
                        locator.contains("corp_code=00126380") && locator.contains("page_no=2")
                    );
                    // Run logs, diagnostics and shared text refuse such a line.
                    assert!(crate::util::credential_text::contains_credential_text(
                        &format!("GET {url} failed")
                    ));
                }
            }
        }
        let public = "https://opendart.fss.or.kr/api/list.json?corp_code=00126380";
        assert_eq!(super::super::public_locator(public), public);
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
