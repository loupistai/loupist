use super::*;

#[test]
fn runtime_tools_require_their_native_vendor_capability() {
    use crate::{agent_harness::AgentRuntimeProfile, broker::tools::TOOL_CATALOG};
    let market = crate::marketplace::bundled_plugins().unwrap();
    // Compare every catalog tool against the previous plugin-level rule.
    // Only DefiLlama under the wallet-only plugin must lose visibility.
    for capabilities in [
        vec!["onchain.wallets".to_owned()],
        vec!["defillama.tvl".to_owned()],
        vec!["onchain.wallets".to_owned(), "defillama.tvl".to_owned()],
        market
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect(),
    ] {
        let runtime = AgentRuntimeProfile::new("chat", true, true, &capabilities).unwrap();
        let names: BTreeSet<_> = runtime
            .core_tool_names
            .iter()
            .chain(
                runtime
                    .components
                    .iter()
                    .flat_map(|component| &component.tool_names),
            )
            .collect();
        let plugins: BTreeSet<_> = market
            .entries
            .iter()
            .filter(|entry| capabilities.contains(&entry.id))
            .map(|entry| &entry.plugin)
            .collect();
        for tool in &TOOL_CATALOG.tools {
            let previously_visible = tool.component.as_ref().is_none_or(|id| {
                TOOL_CATALOG
                    .components
                    .iter()
                    .find(|component| &component.id == id)
                    .unwrap()
                    .connection
                    .as_ref()
                    .is_none_or(|plugin| plugins.contains(plugin))
            });
            let expected = previously_visible
                && !(tool.name == "defillama_read"
                    && !capabilities.iter().any(|id| id == "defillama.tvl"));
            assert_eq!(
                names.contains(&tool.name),
                expected,
                "{} with {capabilities:?}",
                tool.name
            );
        }
    }
}
