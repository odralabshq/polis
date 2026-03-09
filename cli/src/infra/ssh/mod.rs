//! SSH utilities — host key pinning (`KnownHostsManager`) and transport (`SshTransport`).

mod config;
mod identity;
mod known_hosts;
mod sockets;
mod transport;

// Re-export all public types so no call site outside `infra/ssh/` changes.
pub use self::config::*;
pub use self::identity::*;
pub use self::known_hosts::*;
pub use self::sockets::*;
pub use self::transport::*;
