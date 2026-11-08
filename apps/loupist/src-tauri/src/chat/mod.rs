//! Chat turn infrastructure below the command layer: the progress stream, local
//! tool activity, artifacts and charts, the control channel and per-turn resources.

pub(crate) mod activity;
pub(crate) mod approvals;
pub(crate) mod artifacts;
pub(crate) mod chart;
pub(crate) mod command_grants;
pub(crate) mod control;
pub(crate) mod execution_session;
pub(crate) mod grid;
pub(crate) mod handoff;
pub(crate) mod home;
pub(crate) mod progress;
pub(crate) mod questions;
pub(crate) mod surface;
pub(crate) mod turn;
