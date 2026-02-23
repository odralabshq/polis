/// Redis key prefixes for polis state management
pub mod keys {
    /// Blocked requests awaiting approval
    /// Format: polis:blocked:{request_id}
    /// Value: JSON-serialized BlockedRequest
    /// TTL: None (persists until approved/denied)
    pub const BLOCKED: &str = "polis:blocked";

    /// Approved requests (temporary allowlist)
    /// Format: polis:approved:{request_id}
    /// Value: "approved"
    /// TTL: 300 seconds (5 minutes)
    pub const APPROVED: &str = "polis:approved";

    /// Auto-approve configuration rules
    /// Format: polis:config:auto_approve:{pattern}
    /// Value: AutoApproveAction as string
    pub const AUTO_APPROVE: &str = "polis:config:auto_approve";

    /// Global security level setting
    /// Format: polis:config:security_level
    /// Value: SecurityLevel as string
    pub const SECURITY_LEVEL: &str = "polis:config:security_level";

    /// Security event log (sorted set)
    /// Format: polis:log:events
    /// Score: Unix timestamp
    /// Value: JSON-serialized SecurityLogEntry
    pub const EVENT_LOG: &str = "polis:log:events";

    /// OTT (One-Time Token) mappings created by REQMOD code rewriting
    /// Format: polis:ott:{ott_code}
    /// Value: JSON-serialized OttMapping
    /// TTL: 600 seconds (10 minutes — generous window for user to respond)
    pub const OTT_MAPPING: &str = "polis:ott";
}

/// TTL constants
pub mod ttl {
    /// Approved request allowlist TTL (5 minutes)
    pub const APPROVED_REQUEST_SECS: u64 = 300;

    /// Blocked request expiry TTL (1 hour)
    pub const BLOCKED_REQUEST_SECS: u64 = 3600;

    /// OTT mapping TTL (10 minutes — generous window for user to respond)
    pub const OTT_MAPPING_SECS: u64 = 600;

    /// Event log retention (24 hours)
    pub const EVENT_LOG_SECS: u64 = 86400;
}

/// Approval command constants and OTT (One-Time Token) configuration
pub mod approval {
    /// Prefix for the approval command (used in chat and proxy interception)
    pub const APPROVE_PREFIX: &str = "/polis-approve";

    /// Prefix for the deny command
    pub const DENY_PREFIX: &str = "/polis-deny";

    /// Default time-gate duration in seconds.
    /// OTT codes are not valid until this many seconds after REQMOD rewriting.
    /// Prevents self-approval via sendMessage API echo.
    /// Configurable via polis_APPROVAL_TIME_GATE_SECS environment variable.
    pub const DEFAULT_TIME_GATE_SECS: u64 = 15;

    /// OTT code length (must match request_id length for JSON-safe substitution)
    /// Format: "ott-" + 8 alphanumeric chars = 12 chars total
    /// Matches "req-" + 8 hex chars = 12 chars total
    pub const OTT_PREFIX: &str = "ott-";
    pub const OTT_RANDOM_LEN: usize = 8;

    /// Default approval domain allowlist (dot-prefixed for suffix-safe matching).
    /// RESPMOD only scans responses from these domains for approval codes.
    /// Dot-prefix prevents spoofing: ".slack.com" won't match "evil-slack.com".
    /// The RESPMOD scanner MUST enforce dot-boundary matching (CWE-346).
    /// Configurable via polis_APPROVAL_DOMAINS environment variable (comma-separated).
    pub const DEFAULT_APPROVAL_DOMAINS: &[&str] =
        &[".api.telegram.org", ".api.slack.com", ".discord.com"];

    /// Generate the approval command for a given request_id
    pub fn approval_command(request_id: &str) -> String {
        format!("{} {}", APPROVE_PREFIX, request_id)
    }
}

/// Helper functions for key construction
pub fn blocked_key(request_id: &str) -> String {
    format!("{}:{}", keys::BLOCKED, request_id)
}

pub fn approved_key(request_id: &str) -> String {
    format!("{}:{}", keys::APPROVED, request_id)
}

pub fn auto_approve_key(pattern: &str) -> String {
    format!("{}:{}", keys::AUTO_APPROVE, pattern)
}

pub fn ott_key(ott_code: &str) -> String {
    format!("{}:{}", keys::OTT_MAPPING, ott_code)
}

/// Validate that a request_id matches the expected format: req-[a-f0-9]{8}
/// Returns Ok(()) if valid, Err with description if invalid.
/// SECURITY: Always call before constructing Redis keys from untrusted input.
/// Prevents oversized keys, namespace injection, and malformed IDs (CWE-20).
pub fn validate_request_id(request_id: &str) -> Result<(), &'static str> {
    if request_id.len() != 12 {
        return Err("request_id must be exactly 12 characters");
    }
    if !request_id.starts_with("req-") {
        return Err("request_id must start with 'req-'");
    }
    if !request_id[4..]
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("request_id suffix must be lowercase hex [a-f0-9]");
    }
    Ok(())
}

/// Validate that an OTT code matches the expected format: ott-[a-zA-Z0-9]{8}
/// Returns Ok(()) if valid, Err with description if invalid.
pub fn validate_ott_code(ott_code: &str) -> Result<(), &'static str> {
    if ott_code.len() != 12 {
        return Err("OTT code must be exactly 12 characters");
    }
    if !ott_code.starts_with("ott-") {
        return Err("OTT code must start with 'ott-'");
    }
    if !ott_code[4..].chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("OTT code suffix must be alphanumeric [a-zA-Z0-9]");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Key helper output format tests (Requirements 2.3–2.5) ---

    #[test]
    fn blocked_key_format() {
        assert_eq!(blocked_key("req-abc12345"), "polis:blocked:req-abc12345");
    }

    #[test]
    fn approved_key_format() {
        assert_eq!(approved_key("req-abc12345"), "polis:approved:req-abc12345");
    }

    #[test]
    fn auto_approve_key_format() {
        assert_eq!(
            auto_approve_key("*.example.com"),
            "polis:config:auto_approve:*.example.com"
        );
    }

    #[test]
    fn ott_key_format() {
        assert_eq!(ott_key("ott-x7k9m2p4"), "polis:ott:ott-x7k9m2p4");
    }

    // --- Approval command test (Requirements 4.2–4.4) ---

    #[test]
    fn approval_command_output() {
        assert_eq!(
            approval::approval_command("req-abc12345"),
            "/polis-approve req-abc12345"
        );
    }

    // --- DEFAULT_APPROVAL_DOMAINS dot-prefix test (Requirement 4.2) ---

    #[test]
    fn default_approval_domains_dot_prefixed() {
        for domain in approval::DEFAULT_APPROVAL_DOMAINS {
            assert!(
                domain.starts_with('.'),
                "domain {domain:?} must start with '.'"
            );
        }
    }

    // --- validate_request_id tests (Requirements 3.1, 3.3–3.5) ---

    #[test]
    fn validate_request_id_accepts_valid() {
        assert!(validate_request_id("req-abc12345").is_ok());
    }

    #[test]
    fn validate_request_id_rejects_empty() {
        assert!(validate_request_id("").is_err());
    }

    #[test]
    fn validate_request_id_rejects_injection() {
        assert!(validate_request_id("evil:inject").is_err());
    }

    #[test]
    fn validate_request_id_rejects_uppercase_hex() {
        assert!(validate_request_id("req-ABCD1234").is_err());
    }

    #[test]
    fn validate_request_id_rejects_too_short() {
        assert!(validate_request_id("req-abc").is_err());
    }

    #[test]
    fn validate_request_id_rejects_non_hex() {
        assert!(validate_request_id("req-gggggggg").is_err());
        assert!(validate_request_id("req-abc1234g").is_err());
    }

    #[test]
    fn validate_request_id_rejects_symbols_and_spaces() {
        assert!(validate_request_id("req-abc-1234").is_err());
        assert!(validate_request_id("req-abc 1234").is_err());
        assert!(validate_request_id("req-abc_1234").is_err());
    }

    // --- validate_ott_code tests (Requirements 3.2, 3.6–3.7) ---

    #[test]
    fn validate_ott_code_accepts_valid() {
        assert!(validate_ott_code("ott-x7k9m2p4").is_ok());
        assert!(validate_ott_code("ott-12345678").is_ok());
        assert!(validate_ott_code("ott-ABCDEFGH").is_ok());
    }

    #[test]
    fn validate_ott_code_rejects_empty() {
        assert!(validate_ott_code("").is_err());
    }

    #[test]
    fn validate_ott_code_rejects_special_chars_and_spaces() {
        assert!(validate_ott_code("bad-input!!!").is_err());
        assert!(validate_ott_code("ott-abc 1234").is_err());
        assert!(validate_ott_code("ott-abc_1234").is_err());
        assert!(validate_ott_code("ott-abc-1234").is_err());
    }

    #[test]
    fn validate_ott_code_rejects_too_short() {
        assert!(validate_ott_code("ott-abc").is_err());
    }
}
