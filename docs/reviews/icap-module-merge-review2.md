Unified Review: ICAP Module Merge (REQMOD DLP+OTT + RESPMOD ClamAV+OTT)
Reviewer: Unified Review Agent
Date: 2026-02-14
Design Document: design.md

1. Executive Summary
Architecture Style: Inline policy enforcement in proxy adaptation chain (REQMOD/RESPMOD).
Security Posture Score: 6.4/10 (provisional)
Overall Risk Score: 7.8/10 (provisional)
Threat Profile: High-risk internal choke point (all outbound/inbound HTTP adaptation).
OWASP ASI Coverage: ASI01–ASI10 reviewed; primary exposure in ASI02, ASI03, ASI05, ASI07, ASI08.
MITRE ATLAS Tactics: AML.TA0005, AML.TA0010, AML.TA0011, AML.TA0012, AML.TA0014, AML.TA0015.
Compliance Risks: Potential failure of least-privilege and service-auth controls (NIST 800-207 principles).
Verdict: APPROVED WITH CONDITIONS
Critical Unknowns (must be clarified before final risk sign-off)
AuthN/AuthZ for ICAP callers (which principals may invoke credcheck/sentinel_respmod).
Data classification policy for traffic inspected/stored in Valkey/audit logs.
Trust boundary for Host header used in OTT context binding (canonicalization/authenticity guarantees).
2. Critical Findings (MUST FIX)
Finding 1: ICAP service plane has weak caller identity controls (Confused Deputy + spoofing risk)
Severity: CRITICAL
Category: CWE-306, CWE-285, OWASP ASI03/ASI07, MITRE ATLAS AML.T0053
Location: ICAP listener and service wiring

Issue: The design assumes trusted internal callers but does not specify mTLS/service identity for ICAP requests. Current deployment exposes c-icap on 0.0.0.0:1344 with multiple powerful services. Any compromised in-mesh workload can potentially invoke adaptation services as a deputy.

Attack Scenario:

Attacker compromises any container on shared network.
Sends crafted ICAP REQMOD/RESPMOD requests directly to sentinel.
Triggers OTT/approval side effects without intended proxy mediation.
Impact: unauthorized approvals, policy bypass attempts, audit noise, lateral movement.
Evidence:

ICAP listener/service exposure in c-icap.conf:12-46
Current gateway-to-ICAP routing assumptions in g3proxy.yaml:42-55
Zero-trust baseline requires per-session authenticated communication (NIST/OWASP ZTA): https://cheatsheetseries.owasp.org/cheatsheets/Zero_Trust_Architecture_Cheat_Sheet.html
Remediation: Implement Service-to-Service mTLS + SPIFFE-style identity binding between gateway and sentinel, and enforce deny-by-default ICAP ACLs at L3/L4 + application-level caller verification.

Finding 2: Availability collapse risk from fail-closed ClamAV without circuit-breakers
Severity: HIGH
Category: CWE-400, OWASP ASI08, MITRE ATLAS AML.TA0011
Location: merged RESPMOD flow and operational dependencies

Issue: Design requires blocking all responses on clamd failure/timeouts (good for security), but no explicit circuit-breaker/half-open strategy is defined. This turns scanner degradation into full egress outage.

Attack Scenario:

Attacker induces scanner slowdown/outage (resource exhaustion/network partition).
RESPMOD returns 403 for all traffic by policy.
Gateway retries/clients retry; pressure increases.
Impact: cascading failures, outage amplification.
Evidence:

Fail-closed requirement in requirements.md:59-66
Existing health coupling to squidclamav path indicates tight dependency in health.sh:26-31
Circuit-breaker guidance: https://learn.microsoft.com/en-us/azure/architecture/patterns/circuit-breaker
Remediation: Apply Circuit Breaker pattern with quantified thresholds (example: open after 5 failures/30s, half-open probes every 15s, max 3 trial requests), plus bounded queueing and backpressure telemetry.

Finding 3: Large-body scanning blind spots create practical bypass surface
Severity: HIGH
Category: CWE-20/CWE-693, OWASP ASI02/ASI05
Location: existing DLP/approval scan limits carried into merge assumptions

Issue: Current DLP scans first 1MB plus last 10KB; middle region can evade inspection. Approval module skips processing over size limits and non-allowlisted hosts. Merge proposal improves structure but does not fully define anti-evasion strategy for fragmented/multipart/chunk-shifted payloads.

Attack Scenario:

Attacker pads payload >1MB and places sensitive token mid-body.
DLP misses credential (middle window unscanned).
Request passes; downstream abuse proceeds.
Impact: exfiltration and policy bypass.
Evidence:

DLP limits in srv_polis_dlp.c:30-31 and scan behavior srv_polis_dlp.c:908-953
Approval max-body/allowlist gate in srv_polis_approval.c:1000-1048
Quantified impact: At 5MB request size, ~3.99MB can remain uninspected by current first+tail strategy.

Remediation: Use streaming multi-window scan with overlap (fixed-size sliding windows, e.g., 64KB windows with 1KB overlap) and content-type aware parsers for multipart/json/text.

3. Research Findings
ClamAV TCP exposure and command channel hardening
Current Proposal: direct clamd TCP INSTREAM from RESPMOD.
Research Finding: clamd TCP channel is not authenticated by default; network isolation is mandatory.
Recommendation: Modify
References: https://docs.clamav.net/manual/Usage/Scanning.html
ICAP semantics and service separation
Current Proposal: one service per direction due g3proxy constraints.
Research Finding: ICAP model supports separate REQMOD/RESPMOD services; strict service URI separation is recommended.
Recommendation: Keep, but enforce caller identity + OPTIONS capability negotiation.
References: https://www.ietf.org/rfc/rfc3507.txt
Failure containment for security chokepoints
Current Proposal: fail-closed on clamd outage.
Research Finding: fail-closed without breaker/backoff can trigger ASI08-style cascading failure.
Recommendation: Modify
References: https://learn.microsoft.com/en-us/azure/architecture/patterns/circuit-breaker
4. Threat Model (STRIDE)
Component	S	T	R	I	D	E	Notes
REQMOD DLP+OTT	⚠️	⚠️	⚠️	⚠️	⚠️	⚠️	Host-header trust and partial-scan evasion; dual Valkey contexts
RESPMOD ClamAV+OTT	⚠️	⚠️	✅	⚠️	❌	⚠️	Fail-closed AV can cascade outage; OTT replay constraints depend on host integrity
Valkey state plane	⚠️	⚠️	⚠️	⚠️	✅	⚠️	ACLs exist but operational misuse remains high impact
Gateway↔Sentinel ICAP link	❌	⚠️	⚠️	⚠️	⚠️	❌	Missing explicit service-auth pattern in design
Audit log pipeline	✅	⚠️	⚠️	⚠️	✅	✅	Good ZADD usage; ensure immutable retention and signer identity
5. Defense-in-Depth Analysis
Layer	Status	Gap
L0: Process	⚠️	Merge does not explicitly preserve seccomp/exec policy interaction for rewritten payload paths
L1: Governance	⚠️	Split logic in c-ICAP module path risks governance decision drift
L2: Container	✅	Sysbox/runc separation exists; still requires strict inter-service auth
L3: Network	⚠️	ICAP/clamd trust relies on internal network assumptions
L4: MCP	⚠️	Approval side effects from network path not explicitly tied to MCP policy context
6. Failure Mode Analysis
Single Points of Failure
clamd availability: hard fail-closed blocks all responses.
ICAP sentinel process: central adaptation choke point.
Valkey governance contexts: approval state transitions depend on live keyspace access.
Cascading Failure Risks
Scanner timeout storm → global 403 behavior → retry amplification (ASI08).
Healthcheck/config drift from squidclamav to merged service can produce false unhealthy and restart loops.
Missing Circuit Breakers
ClamAV connector (RESPMOD).
Valkey governance reconnect loops under degradation.
Cross-service health checks still pinned to squidclamav in health.sh:26-31.
7. Identified Gaps & Open Questions
Gaps
Fail-Open Defaults anti-pattern: governance-reqmod unavailability skips OTT rewrite silently; define explicit degraded mode telemetry and operator alerts.
Confused Deputy anti-pattern: adaptation services can act on untrusted ICAP callers without strong caller identity.
Context Pollution anti-pattern: Host-derived context binding without canonical trust guarantee.
Open Questions Resolved
Q1: Does current stack assume squidclamav operational coupling?
Answer: Yes.
Rationale: Config/build/health scripts are explicitly squidclamav-oriented: g3proxy.yaml:52-55, Dockerfile:41-99, health.sh:26-31.
Q2: Are least-privilege Valkey ACLs already present?
Answer: Yes, baseline exists.
Rationale: Distinct users and key scopes are defined in valkey_users.acl:2-8.
8. Missing Controls
Mutual service authentication (mTLS) for gateway↔sentinel and sentinel↔state channels.
Circuit breaker + half-open recovery for clamd and Valkey.
Canonical host verification strategy (trusted source, normalization, punycode policy).
End-to-end request correlation IDs across REQMOD/RESPMOD/audit for repudiation defense.
Explicit data classification and retention policy for polis:log:events.
9. Architectural Hardening
Proposed Changes
Add Service Identity Enforcement pattern on ICAP and Valkey paths.
Add Circuit Breaker + Bulkhead pattern around clamd and governance calls.
Add Streaming Validation Gate pattern for large-body inspection windows.
Add Approval Transaction pattern with idempotency keys and signed audit envelopes.
Specific Recommendations
Enforce mTLS with cert rotation ≤24h between gateway and sentinel.
Set clamd protection: timeout 5s connect / 30s stream / breaker 5 failures per 30s / 15s half-open.
Keep SET NX EX for OTT, and validate one-time consumption semantics (already aligned with Redis guidance): https://redis.io/docs/latest/commands/set/
Preserve user confirmation for high-impact tool actions (aligns with ATLAS AML.M0029/AML.M0030).
10. Security Checklist
 No hardcoded secrets
 All inputs validated at trust boundaries
 Fail-closed behavior on errors
 Audit logging for all security decisions
 Rate limiting on all entry points
 mTLS between containers (if applicable)
 Credential rotation strategy defined
 OWASP ASI risks addressed (assessed; mitigations incomplete)
 MITRE ATLAS mitigations applied (partially)
References
design.md
requirements.md
srv_polis_dlp.c:30-31
srv_polis_dlp.c:908-953
srv_polis_approval.c:1000-1048
c-icap.conf:12-46
g3proxy.yaml:42-55
health.sh:26-31
Dockerfile:41-99
valkey_users.acl:2-8
techniques.yaml:1464-1603
techniques.yaml:2097-2110
mitigations.yaml:1140-1475
https://www.ietf.org/rfc/rfc3507.txt
https://docs.clamav.net/manual/Usage/Scanning.html
https://learn.microsoft.com/en-us/azure/architecture/patterns/circuit-breaker
https://cheatsheetseries.owasp.org/cheatsheets/Zero_Trust_Architecture_Cheat_Sheet.html
If you want, I can convert this into a saved report at polis/.kiro/specs/icap-module-merge/unified-review-output.md and add a risk-tracking table mapped to owners and due dates.