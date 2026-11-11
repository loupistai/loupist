//! What the host decides for each broker method, as two tables with one row
//! per method. Neither `match` has a wildcard, so a new `ToolMethod` does not
//! compile until both of its rows choose every column. The turn gates,
//! unattended runs, Memory checks, dataset retention and result provenance
//! read these rows instead of keeping their own lists of methods.
use super::ToolMethod;
use crate::knowledge::Authorship;

/// Host classification of provenance. Model arguments cannot promote derived output to an original.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunResultOrigin {
    AcquiredOriginal,
    WorkspaceFile,
    Discovery,
    Derived,
    SavedContext,
}

/// Whether a call can reach past this computer or outlive the reply. Every
/// `Outbound` call is paused for the rest of a turn that read a
/// custom-connection result, because that server may be the exfiltration
/// target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reach {
    Local,
    Outbound,
}

/// What the third-party-text tier does to the call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AfterText {
    Open,
    /// Standing work outlives the turn and would carry injected instructions forward.
    Paused,
    /// Paused when the call names a custom server, whose tools may write to an outside service.
    CustomPaused,
}

/// Whether the result hands third-party text to the model (and so raises
/// the untrusted-text tier before it is delivered).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResultText {
    Own,
    ThirdParty,
    /// Decided per bundled route (`taint::NUMERIC_ONLY_OPENBB_TOOLS`).
    ByRoute,
}

/// Memory the call stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryWrite {
    NoWrite,
    /// Memory text: refused once a Don't-save result taints the turn and
    /// when the text looks like a credential.
    Text,
    /// A retained tool result or its graph job: refused once a Don't-save
    /// result taints the turn. Not screened for credentials, because a
    /// fetched page may quote an example key.
    Retained,
}

impl MemoryWrite {
    pub(crate) const fn refused_after_dont_save(self) -> bool {
        match self {
            Self::Text | Self::Retained => true,
            Self::NoWrite => false,
        }
    }

    pub(crate) const fn screened_for_credentials(self) -> bool {
        match self {
            Self::Text => true,
            Self::NoWrite | Self::Retained => false,
        }
    }
}

/// Briefings, scheduled runs and delegated hand-offs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Unattended {
    Allowed,
    Refused,
    /// The MCP host checks the captured grant and the read-only Always
    /// approval itself before any I/O.
    GrantChecked,
}

/// Whether text a shared pack added is delimited in the result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackText {
    NoPack,
    Delimited,
}

/// The turn gates and run policy for one method.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ToolSafety {
    pub(crate) reach: Reach,
    pub(crate) after_text: AfterText,
    pub(crate) result_text: ResultText,
    pub(crate) memory: MemoryWrite,
    pub(crate) unattended: Unattended,
    pub(crate) pack_text: PackText,
}

/// How long the host keeps a native result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Retention {
    /// A turn-local run result.
    Turn,
    /// Also a pinned dataset revision of the complete provider result.
    Dataset,
    /// A dataset that an explicit `max_age` reuses instead of calling the
    /// provider again. The tool's schema declares the host-handled `max_age`.
    DatasetMaxAge,
}

impl Retention {
    pub(crate) const fn keeps_dataset(self) -> bool {
        match self {
            Self::Dataset | Self::DatasetMaxAge => true,
            Self::Turn => false,
        }
    }
}

/// The run result a native method records: its runtime id, provenance,
/// who authors a Source saved from it, and how long the host keeps it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeProducer {
    pub(crate) runtime: &'static str,
    pub(crate) origin: RunResultOrigin,
    /// Sharing exports only publisher-authored Sources. Account reads, the
    /// user's files and custom connections are the user's whatever this says
    /// (`saved_authorship`).
    pub(crate) author: Authorship,
    pub(crate) retention: Retention,
}

const fn row(
    reach: Reach,
    after_text: AfterText,
    result_text: ResultText,
    memory: MemoryWrite,
    unattended: Unattended,
    pack_text: PackText,
) -> ToolSafety {
    ToolSafety {
        reach,
        after_text,
        result_text,
        memory,
        unattended,
        pack_text,
    }
}

const fn native(
    runtime: &'static str,
    origin: RunResultOrigin,
    author: Authorship,
    retention: Retention,
) -> Option<NativeProducer> {
    Some(NativeProducer {
        runtime,
        origin,
        author,
        retention,
    })
}

impl ToolMethod {
    /// The method for a model-facing tool name (`run_python`, `web_fetch`).
    pub(crate) fn from_tool_name(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
    }

    /// The model-facing tool name.
    pub(crate) fn tool_name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    #[rustfmt::skip]
    pub(crate) const fn safety(self) -> ToolSafety {
        use AfterText::*;
        use MemoryWrite::*;
        use PackText::*;
        use Reach::*;
        use ResultText::*;
        use Unattended::*;
        match self {
            //                                reach     after text    result      memory        unattended    pack text
            Self::NativeAuthorize      => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleList         => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleRead         => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleCreate       => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleHandoff      => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleWatch        => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleBriefing     => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleUpdate       => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleSetStatus    => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleDelete       => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleRunNow       => row(Outbound, Paused,       Own,        NoWrite,      Refused,      NoPack),
            Self::ScheduleStop         => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ResearchOpen         => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::BotMemory            => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::ChatHistory          => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DatasetDescribe      => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DatasetQuery         => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DatasetExport        => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::DatasetImport        => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::Calculate            => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::WorldbankSeries      => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DefillamaRead        => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::KrxDailyQuote        => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            // Searches the bundled operation catalog; no request leaves the host.
            Self::KisOperationSearch   => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::KisRead              => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartFinancials       => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartFilingSearch     => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartFilingRead       => row(Outbound, Open,         ThirdParty, NoWrite,      Allowed,      NoPack),
            Self::DartCompanyLookup    => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartMajorReport      => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartOwnership        => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::DartDividends        => row(Outbound, Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::McpConnect           => row(Outbound, CustomPaused, Own,        NoWrite,      GrantChecked, NoPack),
            Self::McpCall              => row(Outbound, CustomPaused, ByRoute,    NoWrite,      GrantChecked, NoPack),
            Self::RunResultsList       => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::PlanUpdate           => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::WebSearch            => row(Outbound, Open,         ThirdParty, NoWrite,      Allowed,      NoPack),
            Self::WebFetch             => row(Outbound, Open,         ThirdParty, NoWrite,      Allowed,      NoPack),
            Self::SourceSave           => row(Local,    Open,         Own,        Retained,     Allowed,      NoPack),
            Self::WorldSearch          => row(Local,    Open,         Own,        NoWrite,      Allowed,      Delimited),
            Self::WorldRead            => row(Local,    Open,         Own,        NoWrite,      Allowed,      Delimited),
            Self::NoteSave             => row(Local,    Open,         Own,        Text,         Allowed,      NoPack),
            Self::JournalSave          => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::JournalReview        => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::PrincipleSave        => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::LensFile             => row(Local,    Open,         Own,        Text,         Allowed,      NoPack),
            Self::WorldSchemaUpdate    => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::LoupistSetup         => row(Local,    Open,         Own,        Text,         Refused,      NoPack),
            Self::ConnectionSetup      => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::AskUser              => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::LensSearch           => row(Local,    Open,         Own,        NoWrite,      Allowed,      Delimited),
            Self::LensRead             => row(Local,    Open,         Own,        NoWrite,      Allowed,      Delimited),
            Self::WorldJob             => row(Outbound, Open,         Own,        Retained,     Allowed,      NoPack),
            Self::ConversationMemory   => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::AssetPageRead        => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::ThesisUpdate         => row(Local,    Open,         Own,        Text,         Allowed,      NoPack),
            Self::EarningsSnapshotSave => row(Local,    Open,         Own,        Text,         Allowed,      NoPack),
            Self::ArtifactList         => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::ArtifactRead         => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::ArtifactPublish      => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::ChartQuery           => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
            Self::ChartPublish         => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::GridPublish          => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::WorkspaceCapture     => row(Local,    Open,         Own,        NoWrite,      Refused,      NoPack),
            Self::PythonRun            => row(Local,    Open,         Own,        NoWrite,      Allowed,      NoPack),
        }
    }

    /// The run result a native method records. MCP results name their
    /// server instead (`mcp_host`); a method without a row records none.
    #[rustfmt::skip]
    pub(crate) const fn producer(self) -> Option<NativeProducer> {
        use Authorship::*;
        use Retention::*;
        use RunResultOrigin::*;
        const KNOWLEDGE: &str = "knowledge";
        const FINANCE: &str = "native-finance";
        const WEB: &str = "native-web";
        const ARTIFACTS: &str = "artifact-store";
        const WORKSPACE: &str = "workspace";
        const DATASETS: &str = "dataset-store";
        match self {
            //                                   runtime             origin            author     retention
            Self::WorldSearch          => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::WorldRead            => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::LensSearch           => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::LensRead             => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::ConversationMemory   => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::AssetPageRead        => native(KNOWLEDGE,          SavedContext,     Tool,      Turn),
            Self::WorldbankSeries      => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DefillamaRead        => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::KrxDailyQuote        => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::KisOperationSearch   => native(FINANCE,            Discovery,        User,      Dataset),
            Self::KisRead              => native(FINANCE,            AcquiredOriginal, User,      DatasetMaxAge),
            Self::DartFinancials       => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DartFilingSearch     => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DartFilingRead       => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DartCompanyLookup    => native(FINANCE,            AcquiredOriginal, Publisher, Dataset),
            Self::DartMajorReport      => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DartOwnership        => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::DartDividends        => native(FINANCE,            AcquiredOriginal, Publisher, DatasetMaxAge),
            Self::WebSearch            => native(WEB,                Discovery,        Tool,      Turn),
            Self::WebFetch             => native(WEB,                AcquiredOriginal, Publisher, Turn),
            Self::ArtifactList         => native(ARTIFACTS,          Derived,          Tool,      Turn),
            Self::ArtifactRead         => native(ARTIFACTS,          Derived,          Tool,      Turn),
            Self::ChartQuery           => native(ARTIFACTS,          Derived,          Tool,      Turn),
            Self::WorkspaceCapture     => native(WORKSPACE,          WorkspaceFile,    User,      Turn),
            Self::PythonRun            => native("python-worker",    Derived,          Tool,      Turn),
            Self::DatasetDescribe      => native(DATASETS,           SavedContext,     Tool,      Turn),
            Self::DatasetQuery         => native(DATASETS,           SavedContext,     Tool,      Turn),
            Self::DatasetImport        => native(WORKSPACE,          Derived,          Tool,      Turn),
            Self::Calculate            => native("host-calculation", Derived,          Tool,      Turn),
            Self::NativeAuthorize
            | Self::ScheduleList
            | Self::ScheduleRead
            | Self::ScheduleCreate
            | Self::ScheduleHandoff
            | Self::ScheduleWatch
            | Self::ScheduleBriefing
            | Self::ScheduleUpdate
            | Self::ScheduleSetStatus
            | Self::ScheduleDelete
            | Self::ScheduleRunNow
            | Self::ScheduleStop
            | Self::ResearchOpen
            | Self::BotMemory
            | Self::ChatHistory
            | Self::DatasetExport
            | Self::McpConnect
            | Self::McpCall
            | Self::RunResultsList
            | Self::PlanUpdate
            | Self::SourceSave
            | Self::NoteSave
            | Self::JournalSave
            | Self::JournalReview
            | Self::PrincipleSave
            | Self::LensFile
            | Self::WorldSchemaUpdate
            | Self::LoupistSetup
            | Self::ConnectionSetup
            | Self::AskUser
            | Self::WorldJob
            | Self::ThesisUpdate
            | Self::EarningsSnapshotSave
            | Self::ArtifactPublish
            | Self::ChartPublish
            | Self::GridPublish => None,
        }
    }
}

#[cfg(test)]
mod tests;
