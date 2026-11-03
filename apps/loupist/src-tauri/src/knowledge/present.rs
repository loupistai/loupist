use serde::{Deserialize, Serialize};

use super::model::WorldGraphView;

/// One presentation wrapper. Domain bodies stay free of `originLabel` and
/// `appliedCursor`; those names exist only on this adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Presented<T> {
    pub origin_label: String,
    pub applied_cursor: Option<String>,
    pub body: T,
}

pub fn present_world(
    view: WorldGraphView,
    applied_cursor: Option<String>,
) -> Presented<WorldGraphView> {
    Presented {
        origin_label: "world".to_string(),
        applied_cursor,
        body: view,
    }
}
