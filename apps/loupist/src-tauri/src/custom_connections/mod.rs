//! Host-owned policy and transport for user-configured MCP connections.
//! Server descriptions and annotations are untrusted, including when read-only.

pub mod auth;
pub mod commands;
pub mod config;
mod credentials;
pub mod http;
pub mod policy;
pub mod service;
pub mod taint;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("The connection URL must use HTTPS, or HTTP on localhost or 127.0.0.1.")]
    InvalidUrl,
    #[error("The custom connection returned an invalid or oversized response.")]
    Protocol,
    #[error("The custom connection could not be reached.")]
    Network,
    #[error("The custom connection transport stopped or timed out.")]
    Transport,
    #[error("The custom connection redirected the request; redirects are not allowed.")]
    Redirect,
    #[error("Authorization is required for this connection.")]
    Authorization,
    #[error("Authorization response validation failed.")]
    InvalidAuthorization,
    #[error("Enter a client ID registered with this authorization server under Advanced.")]
    ClientIdRequired,
    #[error("The system credential store could not complete the operation.")]
    CredentialStore,
    #[error("The tool is not approved for this Loupist and connection.")]
    ApprovalRequired,
    #[error("The custom connection configuration is invalid.")]
    Configuration,
}

/// An owner-hosted Client ID Metadata Document is required before enabling CIMD.
pub const CLIENT_METADATA_URL: &str = "";

#[cfg(test)]
pub(crate) mod fixture;
