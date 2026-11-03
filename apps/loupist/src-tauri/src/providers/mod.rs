//! Model providers: the model catalog, user-added endpoints, sign-in, isolated
//! inference and the initial World model choice.

pub(crate) mod auto;
pub(crate) mod catalog;
pub(crate) mod connection;
pub(crate) mod custom;
pub(crate) mod inference;
pub(crate) mod local;
pub(crate) mod status;
pub(crate) mod support;
pub(crate) mod unknown;
pub(crate) mod world_defaults;
