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
    #[error("This feature is not available in this version.")]
    Unavailable,
    #[error("The connection URL must use HTTPS, or HTTP on localhost or 127.0.0.1.")]
    InvalidUrl,
    #[error("The custom connection returned an invalid or oversized response.")]
    Protocol,
    /// A reply over the transport's limit; the server keeps running.
    #[error("The custom connection's reply exceeded the {} MiB limit. The connection is still running.", .limit_bytes / (1024 * 1024))]
    ResultTooLarge { limit_bytes: u64 },
    #[error("The custom connection rejected the tool arguments.")]
    InvalidArguments,
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
    #[error("A saved-key operation for this connection is still waiting for macOS.")]
    KeychainBusy,
    /// Another conversation's call or a Settings action holds the connection.
    /// The model reads this with `mcp_host::custom`'s next line.
    #[error("This connection is in use by another request, so this call was not sent. Its key and setup are fine.")]
    InUse,
    #[error("The Keychain did not respond in time.")]
    KeychainUnavailable,
    #[error("The Keychain did not respond in time.")]
    KeychainMutationPending,
    #[error("Loupist is installing an update")]
    UpdateInstalling,
    #[error("The tool is not approved for this Loupist and connection.")]
    ApprovalRequired,
    #[error("The custom connection configuration is invalid.")]
    Configuration,
}

/// An owner-hosted Client ID Metadata Document is required before enabling CIMD.
pub const CLIENT_METADATA_URL: &str = "";

#[cfg(test)]
pub(crate) mod fixture;
