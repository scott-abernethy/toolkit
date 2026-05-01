use std::fmt;

pub type Result<T> = std::result::Result<T, ToolkitError>;

/// Categorised toolkit error. The variant captures *what kind* of failure
/// occurred so callers can branch on it; the inner string is the agent-facing
/// message (already sanitised by the producer).
#[derive(Debug, Clone)]
pub enum ToolkitError {
    /// Configuration is missing, malformed, or a section is absent.
    Config(String),
    /// Connection to a backend (database, network) failed.
    Connection(String),
    /// Authentication failed at a backend.
    Auth(String),
    /// A resource (table, profile, connection name, file) doesn't exist.
    NotFound(String),
    /// Caller's privileges are insufficient at the backend.
    Permission(String),
    /// A write was attempted but the policy denies it.
    WriteDenied(String),
    /// A wrapped CLI failed in a way we couldn't classify.
    Cli(String),
    /// Daemon not reachable via UNIX socket (connection refused / no such file).
    Daemon(String),
    /// Catch-all for cases that don't fit the other variants.
    Other(String),
}

impl ToolkitError {
    /// Agent-facing message (the value placed in `{"error": "..."}`).
    pub fn message(&self) -> &str {
        match self {
            Self::Config(m)
            | Self::Connection(m)
            | Self::Auth(m)
            | Self::NotFound(m)
            | Self::Permission(m)
            | Self::WriteDenied(m)
            | Self::Cli(m)
            | Self::Daemon(m)
            | Self::Other(m) => m,
        }
    }

    /// Stable variant tag for audit logs and metrics. Never includes the
    /// inner message — only the category, so it's safe to aggregate on.
    pub fn class(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Connection(_) => "connection",
            Self::Auth(_) => "auth",
            Self::NotFound(_) => "not_found",
            Self::Permission(_) => "permission",
            Self::WriteDenied(_) => "write_denied",
            Self::Cli(_) => "cli",
            Self::Daemon(_) => "daemon",
            Self::Other(_) => "other",
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn permission(msg: impl Into<String>) -> Self {
        Self::Permission(msg.into())
    }
    pub fn write_denied(msg: impl Into<String>) -> Self {
        Self::WriteDenied(msg.into())
    }
    pub fn cli(msg: impl Into<String>) -> Self {
        Self::Cli(msg.into())
    }
    pub fn daemon(msg: impl Into<String>) -> Self {
        Self::Daemon(msg.into())
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

impl fmt::Display for ToolkitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ToolkitError {}
