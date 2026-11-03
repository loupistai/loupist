//! Run admission and lifetime: model-worker coordination, maintenance
//! exclusion, run ids, private scratch directories and content-free run logs.

pub(crate) mod coordinator;
pub(crate) mod id;
pub(crate) mod log;
pub(crate) mod maintenance;
pub(crate) mod scratch;
