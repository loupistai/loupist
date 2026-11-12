use super::*;
use crate::broker::tools::{ToolPermission, TOOL_CATALOG};
use ToolMethod as M;

/// Every broker method: the catalog's routes and the host-only routes.
fn all_methods() -> Vec<ToolMethod> {
    let mut methods: Vec<ToolMethod> = TOOL_CATALOG
        .tools
        .iter()
        .filter_map(|tool| tool.method.as_deref())
        .map(|route| ToolMethod::parse(route).expect("catalog route"))
        .collect();
    methods.extend([M::NativeAuthorize, M::McpConnect, M::McpCall]);
    for (index, method) in methods.iter().enumerate() {
        assert!(!methods[..index].contains(method), "{method:?} twice");
    }
    methods
}

fn catalog_declares_max_age(method: ToolMethod) -> bool {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../../agent/tool-catalog.json")).unwrap();
    catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == method.tool_name())
        .is_some_and(|tool| tool.pointer("/parameters/properties/max_age").is_some())
}

const NATIVE_FINANCE_READS: [ToolMethod; 12] = [
    M::WorldbankSeries,
    M::DefillamaRead,
    M::KrxDailyQuote,
    M::KisOperationSearch,
    M::KisRead,
    M::DartFinancials,
    M::DartFilingSearch,
    M::DartFilingRead,
    M::DartCompanyLookup,
    M::DartMajorReport,
    M::DartOwnership,
    M::DartDividends,
];
const SCHEDULE_CHANGES: [ToolMethod; 7] = [
    M::ScheduleCreate,
    M::ScheduleHandoff,
    M::ScheduleWatch,
    M::ScheduleBriefing,
    M::ScheduleUpdate,
    M::ScheduleRunNow,
    M::ScheduleSetStatus,
];

#[test]
fn every_method_keeps_its_reviewed_turn_gates_and_run_policy() {
    let outbound: Vec<ToolMethod> = NATIVE_FINANCE_READS
        .into_iter()
        // The KIS operation search reads the bundled catalog only.
        .filter(|method| *method != M::KisOperationSearch)
        .chain(SCHEDULE_CHANGES)
        .chain([
            M::McpConnect,
            M::McpCall,
            M::WebSearch,
            M::WebFetch,
            M::WorldJob,
        ])
        .collect();
    let unattended = [
        "chat_history",
        "dataset_describe",
        "dataset_query",
        "calculate",
        "worldbank_series",
        "defillama_read",
        "krx_daily_quote",
        "kis_operation_search",
        "kis_read",
        "dart_financials",
        "dart_filing_search",
        "dart_filing_read",
        "dart_company_lookup",
        "dart_major_report",
        "dart_ownership",
        "dart_dividends",
        "run_results_list",
        "plan_update",
        "web_search",
        "web_fetch",
        "source_save",
        "world_search",
        "world_read",
        "note_save",
        "lens_search",
        "lens_read",
        "lens_file",
        "world_job",
        "conversation_memory",
        "asset_page_read",
        "thesis_update",
        "earnings_snapshot_save",
        "artifact_list",
        "artifact_read",
        "chart_query",
        "run_python",
    ];
    let refused_after_dont_save = [
        M::SourceSave,
        M::NoteSave,
        M::JournalSave,
        M::JournalReview,
        M::PrincipleSave,
        M::LensFile,
        M::LoupistSetup,
        M::WorldJob,
        M::BotMemory,
        M::WorldSchemaUpdate,
        M::ThesisUpdate,
        M::EarningsSnapshotSave,
    ];
    let credential_screened = [
        M::NoteSave,
        M::BotMemory,
        M::PrincipleSave,
        M::LensFile,
        M::WorldSchemaUpdate,
        M::LoupistSetup,
        M::JournalSave,
        M::JournalReview,
        M::ThesisUpdate,
        M::EarningsSnapshotSave,
    ];
    let pack_text = [M::LensRead, M::LensSearch, M::WorldRead, M::WorldSearch];
    for method in all_methods() {
        let class = method.safety();
        let mcp = matches!(method, M::McpConnect | M::McpCall);
        assert_eq!(
            class.reach == Reach::Outbound,
            outbound.contains(&method),
            "{method:?} reach"
        );
        let after_text = if mcp {
            AfterText::CustomPaused
        } else if SCHEDULE_CHANGES.contains(&method)
            || matches!(method, M::ScheduleDelete | M::ScheduleStop)
        {
            AfterText::Paused
        } else {
            AfterText::Open
        };
        assert_eq!(class.after_text, after_text, "{method:?} after text");
        let result_text = match method {
            M::WebSearch | M::WebFetch | M::DartFilingRead => ResultText::ThirdParty,
            M::McpCall => ResultText::ByRoute,
            _ => ResultText::Own,
        };
        assert_eq!(class.result_text, result_text, "{method:?} result text");
        assert_eq!(
            class.memory.refused_after_dont_save(),
            refused_after_dont_save.contains(&method),
            "{method:?} Don't save"
        );
        assert_eq!(
            class.memory.screened_for_credentials(),
            credential_screened.contains(&method),
            "{method:?} credential screen"
        );
        let run = if mcp {
            Unattended::GrantChecked
        } else if unattended.contains(&method.tool_name().as_str()) {
            Unattended::Allowed
        } else {
            Unattended::Refused
        };
        assert_eq!(class.unattended, run, "{method:?} unattended");
        assert_eq!(
            class.pack_text == PackText::Delimited,
            pack_text.contains(&method),
            "{method:?} pack text"
        );
    }
}

#[test]
fn every_method_keeps_its_reviewed_provenance_and_retention() {
    use RunResultOrigin::*;
    let producers = [
        (M::WorldSearch, "knowledge", SavedContext),
        (M::WorldRead, "knowledge", SavedContext),
        (M::LensSearch, "knowledge", SavedContext),
        (M::LensRead, "knowledge", SavedContext),
        (M::ConversationMemory, "knowledge", SavedContext),
        (M::AssetPageRead, "knowledge", SavedContext),
        (M::WorldbankSeries, "native-finance", AcquiredOriginal),
        (M::DefillamaRead, "native-finance", AcquiredOriginal),
        (M::KrxDailyQuote, "native-finance", AcquiredOriginal),
        (M::KisOperationSearch, "native-finance", Discovery),
        (M::KisRead, "native-finance", AcquiredOriginal),
        (M::DartFinancials, "native-finance", AcquiredOriginal),
        (M::DartFilingSearch, "native-finance", AcquiredOriginal),
        (M::DartFilingRead, "native-finance", AcquiredOriginal),
        (M::DartCompanyLookup, "native-finance", AcquiredOriginal),
        (M::DartMajorReport, "native-finance", AcquiredOriginal),
        (M::DartOwnership, "native-finance", AcquiredOriginal),
        (M::DartDividends, "native-finance", AcquiredOriginal),
        (M::WebSearch, "native-web", Discovery),
        (M::WebFetch, "native-web", AcquiredOriginal),
        (M::ArtifactList, "artifact-store", Derived),
        (M::ArtifactRead, "artifact-store", Derived),
        (M::ChartQuery, "artifact-store", Derived),
        (M::WorkspaceCapture, "workspace", WorkspaceFile),
        (M::PythonRun, "python-worker", Derived),
        (M::DatasetDescribe, "dataset-store", SavedContext),
        (M::DatasetQuery, "dataset-store", SavedContext),
        (M::DatasetImport, "workspace", Derived),
        (M::Calculate, "host-calculation", Derived),
    ];
    let publisher = [
        M::WebFetch,
        M::DartFinancials,
        M::DartFilingRead,
        M::DartFilingSearch,
        M::DartCompanyLookup,
        M::DartMajorReport,
        M::DartOwnership,
        M::DartDividends,
        M::KrxDailyQuote,
        M::WorldbankSeries,
        M::DefillamaRead,
    ];
    for method in all_methods() {
        let expected = producers.iter().find(|(listed, ..)| *listed == method);
        let Some(producer) = method.producer() else {
            assert!(expected.is_none(), "{method:?} lost its producer");
            continue;
        };
        let (_, runtime, origin) = expected.expect("a new producer is listed here");
        assert_eq!(producer.runtime, *runtime, "{method:?}");
        assert_eq!(producer.origin, *origin, "{method:?}");
        let author = if publisher.contains(&method) {
            Authorship::Publisher
        } else if matches!(
            method,
            M::KisOperationSearch | M::KisRead | M::WorkspaceCapture
        ) {
            Authorship::User
        } else {
            Authorship::Tool
        };
        assert_eq!(producer.author, author, "{method:?} author");
        let retention = if !NATIVE_FINANCE_READS.contains(&method) {
            Retention::Turn
        } else if matches!(method, M::KisOperationSearch | M::DartCompanyLookup) {
            Retention::Dataset
        } else {
            Retention::DatasetMaxAge
        };
        assert_eq!(producer.retention, retention, "{method:?} retention");
        assert_eq!(
            ToolMethod::from_tool_name(&method.tool_name()),
            Some(method)
        );
    }
}

/// Invariants between columns. Each would have caught a read added to one
/// list and forgotten in another.
#[test]
fn classification_columns_agree_with_each_other_and_the_catalog() {
    for method in all_methods() {
        let safety = method.safety();
        let producer = method.producer();
        let max_age = producer.is_some_and(|p| p.retention == Retention::DatasetMaxAge);
        // The host takes `max_age` only from schemas that declare it.
        assert_eq!(max_age, catalog_declares_max_age(method), "{method:?}");
        // An original acquired from a provider or a page reached the network,
        // so a turn that read a custom-connection result pauses it.
        if producer.is_some_and(|p| p.origin == RunResultOrigin::AcquiredOriginal) {
            assert_eq!(safety.reach, Reach::Outbound, "{method:?}");
        }
        // Every Memory write needs a Memory permission, and the reverse.
        let permission = match method {
            M::NativeAuthorize | M::McpConnect | M::McpCall => ToolPermission::Always,
            method => method.permission(),
        };
        if permission == ToolPermission::MemoryWrite {
            assert_ne!(safety.memory, MemoryWrite::NoWrite, "{method:?}");
        }
        if safety.memory != MemoryWrite::NoWrite {
            assert_ne!(permission, ToolPermission::Always, "{method:?}");
        }
        // A briefing starts at the untrusted-text tier, so a tool that tier
        // pauses could never run unattended.
        if safety.unattended == Unattended::Allowed {
            assert_eq!(safety.after_text, AfterText::Open, "{method:?}");
        }
    }
}
