//! Data models for output rendering.
//!
//! These types are used by the Renderer to produce both human-readable
//! and JSON output. They are presentation-layer types that may wrap or
//! transform domain/application types for output purposes.

use serde::Serialize;

use crate::application::services::security::PendingResult;

/// Connection information for IDE integration.
#[derive(Debug, Serialize)]
pub struct ConnectionInfo {
    /// SSH connection command.
    pub ssh: String,
    /// VS Code remote connection command.
    pub vscode: String,
    /// Cursor remote connection command.
    pub cursor: String,
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        Self {
            ssh: "ssh workspace".to_string(),
            vscode: "code --remote ssh-remote+workspace /workspace".to_string(),
            cursor: "cursor --remote ssh-remote+workspace /workspace".to_string(),
        }
    }
}

/// Security status for JSON output.
#[derive(Debug, Serialize)]
pub struct SecurityStatus {
    /// Current security level.
    pub level: String,
    /// Number of pending blocked requests.
    pub pending_count: usize,
    /// Error message if pending requests could not be queried.
    pub pending_error: Option<String>,
}

impl SecurityStatus {
    /// Convert from service `SecurityStatus` to output model `SecurityStatus`.
    #[must_use]
    pub fn from_service(s: &crate::application::services::security::SecurityStatus) -> Self {
        let (pending_count, pending_error) = match &s.pending {
            PendingResult::Ok(requests) => (requests.len(), None),
            PendingResult::Err(err) => (0, Some(err.clone())),
        };
        Self {
            level: s.level.to_string(),
            pending_count,
            pending_error,
        }
    }
}

/// Pending security request for JSON output.
#[derive(Debug, Serialize)]
pub struct PendingRequest {
    /// Request identifier.
    pub id: String,
    /// Domain of the request.
    pub domain: String,
    /// Timestamp of the request.
    pub timestamp: String,
}

impl PendingRequest {
    /// Parse pending request lines into structured `PendingRequest` objects.
    ///
    /// The service returns raw string lines. This function attempts to parse them
    /// into structured data. If parsing fails, it creates a fallback entry with
    /// the raw line as the domain.
    #[must_use]
    pub fn parse_lines(lines: &[String]) -> Vec<Self> {
        lines
            .iter()
            .map(|line| {
                // Attempt to parse "id - domain (timestamp)" format
                // Fallback: use the whole line as domain if parsing fails
                let parts: Vec<&str> = line.splitn(3, " - ").collect();
                if parts.len() >= 2 {
                    let id = parts[0].trim().to_string();
                    let rest = parts[1];
                    // Try to extract timestamp from parentheses
                    if let Some(paren_start) = rest.rfind('(') {
                        let domain = rest[..paren_start].trim().to_string();
                        let timestamp = rest[paren_start + 1..]
                            .trim_end_matches(')')
                            .trim()
                            .to_string();
                        Self {
                            id,
                            domain,
                            timestamp,
                        }
                    } else {
                        Self {
                            id,
                            domain: rest.trim().to_string(),
                            timestamp: String::new(),
                        }
                    }
                } else {
                    // Fallback: use the whole line
                    Self {
                        id: String::new(),
                        domain: line.clone(),
                        timestamp: String::new(),
                    }
                }
            })
            .collect()
    }
}

/// Security log entry for JSON output.
#[derive(Debug, Serialize)]
pub struct LogEntry {
    /// Timestamp of the log entry.
    pub timestamp: String,
    /// Action taken.
    pub action: String,
    /// Additional details.
    pub details: String,
}

impl LogEntry {
    /// Parse log entry lines into structured `LogEntry` objects.
    ///
    /// The service returns raw string lines. This function attempts to parse them
    /// into structured data. If parsing fails, it creates a fallback entry with
    /// the raw line as details.
    #[must_use]
    pub fn parse_lines(lines: &[String]) -> Vec<Self> {
        lines
            .iter()
            .map(|line| {
                // Attempt to parse "[timestamp] action - details" format
                // Fallback: use the whole line as details if parsing fails
                if line.starts_with('[')
                    && let Some(bracket_end) = line.find(']')
                {
                    let timestamp = line[1..bracket_end].to_string();
                    let rest = line[bracket_end + 1..].trim();
                    let parts: Vec<&str> = rest.splitn(2, " - ").collect();
                    if parts.len() >= 2 {
                        return Self {
                            timestamp,
                            action: parts[0].trim().to_string(),
                            details: parts[1].trim().to_string(),
                        };
                    }
                    return Self {
                        timestamp,
                        action: rest.to_string(),
                        details: String::new(),
                    };
                }
                // Fallback: use the whole line as details
                Self {
                    timestamp: String::new(),
                    action: String::new(),
                    details: line.clone(),
                }
            })
            .collect()
    }
}
