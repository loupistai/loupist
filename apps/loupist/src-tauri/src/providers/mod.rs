//! Model providers: the model catalog, user-added endpoints, sign-in, isolated
//! inference and the World model the bundled list recommends.

pub(crate) mod auto;
pub(crate) mod catalog;
pub(crate) mod connection;
pub(crate) mod custom;
pub(crate) mod inference;
pub(crate) mod local;
pub(crate) mod manifest;
pub(crate) mod openrouter_quota;
pub(crate) mod selection;
pub(crate) mod status;
pub(crate) mod support;
pub(crate) mod unknown;
pub(crate) mod world_defaults;
pub(crate) mod world_model_list;
