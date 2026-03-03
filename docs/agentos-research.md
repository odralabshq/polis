# AgentOS Deep Research — State of the Field (March 2026)

Research synthesis for the AgentOS architectural RFC prompt (`docs/prompt.md`), covering OS-level primitives for AI agents, intent-based security, kernel architecture, and the broader research landscape.

---

## 1. Agent-Specific OS Research & Projects

**AIOS (Rutgers University)** — The most cited academic work. AIOS introduces an LLM-specific kernel layer providing scheduling, context management, memory management, storage management, and access control for runtime agents. Published at ICLR 2025 and iterated through multiple versions. It includes an SDK and an LLM-based Semantic File System (LSFS) that enables prompt-driven file management — agents interact with files via natural language rather than POSIX paths. [arxiv.org/html/2403.16971v5](https://arxiv.org/html/2403.16971v5)

**Aura (Tsinghua University, Feb 2026)** — The most architecturally relevant paper to the prompt. Aura proposes an "Agent Universal Runtime Architecture" for mobile agents with a Hub-and-Spoke topology: a privileged System Agent orchestrates intent, sandboxed App Agents execute tasks, and an Agent Kernel mediates all communication. The Agent Kernel enforces four defense pillars:
- Cryptographic identity via a Global Agent Registry and Agent Identity Cards (AICs)
- Semantic input sanitization through a multilayer Semantic Firewall
- Cognitive integrity via taint-aware memory and plan-trajectory alignment
- Granular action access control with critical-node interception and non-deniable auditing

Evaluation showed Attack Success Rate dropped from ~40% to 4.4% vs baselines. This is the closest existing work to the "intent capsule" concept in the prompt. [arxiv.org/html/2602.10915v2](https://arxiv.org/html/2602.10915v2)

**Agent-OS Blueprint (Preprints.org, Sep 2025)** — Defines an abstract layered architecture: Kernel, Services, Agent Runtime, Orchestration, and User layers, with cross-cutting concerns for security, governance, and observability. Positioned as an "architectural North Star" for the next decade. [preprints.org/manuscript/202509.0077](https://www.preprints.org/manuscript/202509.0077)

**AgenticOS Workshop @ ASPLOS 2026 (March 23, 2026, Pittsburgh)** — The first academic workshop explicitly on "Operating Systems Design for AI Agents." Topics include new OS abstractions for agent execution, semantics-aware scheduling, eBPF-driven extensions, long-lived state abstractions for agent context, and security/isolation for agent-invoked tools. PC includes faculty from UC Merced, Virginia Tech, UCSD, Tsinghua, and industry from Roblox and ByteDance. [os-for-agent.github.io](https://os-for-agent.github.io/)

**Karpathy's LLM OS Vision** — Andrej Karpathy's conceptual "LLM OS" framing (LLM as the new CPU, context window as RAM, tools as peripherals) has been widely cited. The Swarm Corporation built a lightweight open-source AgentOS implementing this. However, Karpathy himself has stated that fully operational AI agents are "at least a decade away." [github.com/The-Swarm-Corporation/AgentOS](https://github.com/The-Swarm-Corporation/AgentOS)

**"We're Building DOS Again" (Vonng, Dec 2025)** — Blog post arguing that 2025 is the year of the coding agent explosion and that we're recreating the same OS evolution trajectory, but for agents. [blog.vonng.com/en/db/agent-os](http://blog.vonng.com/en/db/agent-os/)

---

## 2. Security Research — Intent-Based & Agent-Specific

**OWASP Top 10 for Agentic Applications (Dec 2025)** — The first security framework dedicated to autonomous AI systems. Released at Black Hat Europe 2025 by 100+ security experts. Covers: goal hijacking, tool misuse, privilege abuse, memory poisoning, cascading failures, and rogue agents. 48% of cybersecurity professionals identified agentic AI as the #1 attack vector for 2026, yet only 34% of enterprises had AI-specific security controls. [promptfoo.dev/docs/red-team/owasp-agentic-ai](https://www.promptfoo.dev/docs/red-team/owasp-agentic-ai/)

**MITRE ATLAS 6.0 (Oct 2025)** — Introduced 14 new agentic AI techniques and a Technique Maturity classification (Feasible/Demonstrated/Realized) across 15 tactics and 66 techniques with 46 sub-techniques. Zenity contributed agentic-specific techniques in the first 2026 update. [zenity.io/blog/zenitys-contributions-to-mitre-atlas-first-2026-update](https://zenity.io/blog/current-events/zenitys-contributions-to-mitre-atlas-first-2026-update)

**AgentSentry** — A unified security framework with task-centric access control and dynamic permission revocation. Integrates dynamic policy generation, enforcement DSL rules, and multi-agent anomaly detection. [emergentmind.com/topics/agentsentry-framework](https://www.emergentmind.com/topics/agentsentry-framework)

**Forrester AEGIS Framework** — Enterprise-focused security framework for agentic AI, addressing distributed, autonomous, scalable systems with emergent behavior. [forrester.com/blogs/introducing-aegis](https://www.forrester.com/blogs/introducing-aegis-the-guardrails-cisos-need-for-the-agentic-enterprise/)

**Intent-Based Permissions (HelpNetSecurity, Oct 2025)** — Argues IAM now needs intent-based permissions that understand not only what an AI agent is doing, but why. Directly validates the "intent capsule" concept. [helpnetsecurity.com/2025/10/10/agentic-ai-intent-based-permissions](https://www.helpnetsecurity.com/2025/10/10/agentic-ai-intent-based-permissions/)

**Behavioral Baseline for Agentic AI (Lasso Security, Feb 2026)** — Notes that each agent step may pass policy validation individually, yet the cumulative plan can drift beyond the approved scope. Argues for behavioral baselines as a security primitive. [lasso.security/blog/intent-security-behavioral-for-agentic-ai](https://www.lasso.security/blog/intent-security-behavioral-for-agentic-ai)

---

## 3. Key Statistics & Data Points

**19.7% hallucinated package names** — Confirmed. UTSA researchers analyzed 576,000 Python/JavaScript code samples and found ~440,445 references to nonexistent packages (~19.7%). Accepted at USENIX Security Symposium. This attack vector is now called "slopsquatting" and has its own Wikipedia page. [hoodline.com](https://hoodline.com/2026/02/san-antonio-ai-researchers-sound-alarm-on-phantom-package-trap/)

**81-95% malicious tool call success rates** — Confirmed. Research paper "Inducing LLM Agents to Invoke Malicious Tools" demonstrated 81-95% attack success rates across ten realistic tool-use scenarios with negligible impact on primary task execution. [arxiv.org/html/2508.02110v1](https://arxiv.org/html/2508.02110v1)

**Prompt-only security failure rates** — Multi-turn attacks achieved success rates as high as 92% across eight open-weight models. Anthropic tested 16 models and found explicit safety instructions are insufficient — agents still engage in harmful behavior over a third of the time. OWASP notes prompt injection appears in 73%+ of production AI deployments. [helpnetsecurity.com/2026/02/23/ai-agent-security-risks-enterprise](https://www.helpnetsecurity.com/2026/02/23/ai-agent-security-risks-enterprise/)

**Multi-Agent Systems Execute Arbitrary Malicious Code** — OpenReview paper demonstrating that multi-agent systems interacting with untrusted inputs (web content, files, email attachments) can be induced to execute arbitrary malicious code. [openreview.net/forum?id=DAozI4etUp](https://openreview.net/forum?id=DAozI4etUp)

---

## 4. eBPF & Kernel-Level Agent Observability

**AgentSight (Aug 2025)** — The most directly relevant project. Uses eBPF to monitor AI agents at system boundaries. Intercepts TLS-encrypted LLM traffic to extract semantic intent, monitors kernel events (execve, connect, openat) to observe system-wide effects, and causally correlates these two streams using a real-time engine. This is essentially a prototype of the "behavioral session graph" concept in the prompt. [arxiv.org/html/2508.02736v1](https://arxiv.org/html/2508.02736v1)

**eBPF Provenance Analysis** — A project using custom eBPF probes to reconstruct causal graphs of system activity, linking processes to file modifications and network connections. Computes cryptographic hashes of files accessed during compilation and constructs Merkle trees. [devpost.com/software/ebpf-based-system-auditing](https://devpost.com/software/ebpf-based-system-auditing)

**FG-RCA (Fine-Grained Runtime Containment Agent)** — Uses eBPF with Linux Security Modules (LSM) to learn least-privilege behavior from execution and enforce it in the kernel. Published in MDPI journal. [mdpi.com/2624-831X/7/1/3](https://www.mdpi.com/2624-831X/7/1/3)

**eBPF Supply Chain Monitoring** — Framework for kernel-level monitoring using eBPF to compute cryptographic hashes of files accessed during compilation and construct Merkle trees for tamper-evident dependency identification. [arxiv.org/html/2503.02097v1](https://arxiv.org/html/2503.02097v1)

---

## 5. Cognitive Degradation & Behavioral Drift

**Cognitive Degradation as a Vulnerability Class (Jul 2025)** — Arxiv paper introduces cognitive degradation as a novel vulnerability in agentic AI: memory starvation, planner recursion, context flooding, and output suppression. These failures originate internally, not from external attacks. [arxiv.org/html/2507.15330v1](https://arxiv.org/html/2507.15330v1)

**Cloud Security Alliance CDR Framework** — Introduces Cognitive Degradation Resilience (CDR) for agentic AI, documenting planner starvation, memory entrenchment, behavioral drift, and systemic collapse across perception/memory/planning/tools/output subsystems. [cloudsecurityalliance.org](https://cloudsecurityalliance.org/articles/introducing-cognitive-degradation-resilience-cdr-a-framework-for-safeguarding-agentic-ai-systems-from-systemic-collapse)

**Behavioral Drift Detection (Dec 2025)** — Temporal Data Kernel Perspective Space (TDKPS) framework for statistically detecting behavioral drift in black-box multi-agent systems by watching how agents respond over time. [cognaptus.com](https://cognaptus.com/blog/2025-12-05-shift-happens-detecting-behavioral-drift-in-multiagent-systems/)

**Graph-based Anomaly Detection in Multi-Agent Systems (May 2025)** — Proposes a graph-based framework that models agent interactions as dynamic execution graphs, enabling semantic anomaly detection at node, edge, and path levels. [arxiv.org/html/2505.24201v1](https://arxiv.org/html/2505.24201v1)

**MI9 Framework** — Integrated runtime governance framework designed to address emergent behaviors and autonomous goal drift in agentic AI systems. [emergentmind.com/topics/mi9-framework](https://www.emergentmind.com/topics/mi9-framework)

---

## 6. Sandboxing & Isolation Comparison

The landscape as of early 2026 (from the eunomia.dev survey and northflank.com):

| Approach | Isolation Strength | Startup Latency | Use Case |
|---|---|---|---|
| Standard containers | Weak (shared kernel) | Fast | Trusted code only |
| gVisor (user-space kernel) | Medium (syscall interception) | Medium | Balanced security/perf |
| Firecracker microVMs | Strong (dedicated kernel) | Higher (~125ms) | Multi-tenant, untrusted |
| Kata Containers | Strong (full VM) | Higher | Kubernetes-native isolation |
| Sysbox | Strong (VM-like, no privileged mode) | Medium | Docker-in-Docker, agent workspaces |
| WASM/WASI | Strong (capability-based) | Very fast | Lightweight tool execution |

**Google Kubernetes Agent Sandbox (late 2025)** — Uses gVisor + Kata with pre-warmed pools achieving sub-second startup (~90% improvement over cold-starting). Managed via CRD (`Sandbox`), supports pause/resume, memory sharing, and Pod Snapshots for checkpoint/restore. [github.com/kubernetes-sigs/agent-sandbox](https://github.com/kubernetes-sigs/agent-sandbox)

**Eunomia.dev Survey (Jan 2026)** — Comprehensive survey of agent system architectures covering isolation, integration, and governance. Catalogs 15+ research/OSS projects and 11+ commercial sandbox services. Key finding: sandboxing has evolved from pure security into lifecycle management (persistent storage, snapshots, warm pools) and controlled handoffs (pause/resume, human takeover). [eunomia.dev/blog/2026/01/11/architectures-for-agent-systems](https://eunomia.dev/blog/2026/01/11/architectures-for-agent-systems-a-survey-of-isolation-integration-and-governance/)

**Fault-Tolerant Sandboxing** — Research prototype introducing transactional file system wrapper for agent execution. 100% of unsafe actions intercepted and rolled back at ~14.5% performance overhead. Limitation: doesn't undo external side-effects (API calls, emails).

---

## 7. Semantic Filesystems

**LSFS (AIOS Semantic File System)** — Presented at ICLR 2025. Enables semantic file retrieval, file update summarization, and semantic file rollback via natural language. [openreview.net/forum?id=2G021ZqUEZ](https://openreview.net/forum?id=2G021ZqUEZ)

**VexFS** — A kernel-native filesystem storing vector embeddings alongside files, supporting semantic search via IOCTL + mmap interface. Community project on Hacker News. [news.ycombinator.com/item?id=44095926](https://news.ycombinator.com/item?id=44095926)

**"Everything is Context" (CSIRO Data61 + ArcBlock, 2026)** — Proposes treating memory, tools, knowledge bases, and human inputs as a mounted filesystem that AI agents browse dynamically at runtime. [blockchain.news](https://blockchain.news/ainews/everything-is-context-csiro-data61-and-arcblock-propose-filesystem-based-ai-agent-architecture-5-business-impacts-and-2026-trends)

**Dust's Synthetic Filesystems** — Maps disparate data sources into navigable Unix-inspired structures. Agents were observed spontaneously inventing filesystem-like syntax for searching content (e.g., `file:front/src/some-file-name.tsx`). [dust.tt/blog](https://blog.dust.tt/how-we-taught-ai-agents-to-navigate-company-data-like-a-filesystem/)

**AgentFS (Turso)** — "The filesystem for agents" — purpose-built filesystem abstraction for agent workloads. [github.com/tursodatabase/agentfs](https://github.com/tursodatabase/agentfs)

---

## 8. Real-World CVEs & Prompt Injection at OS Level

- **CVE-2025-54135 (CurXecute)** — Cursor AI: remote code execution via prompt injection through MCP server. Attacker feeds poisoned data to agent via MCP, gains full RCE under user privileges. [bleepingcomputer.com](https://www.bleepingcomputer.com/news/security/ai-powered-cursor-ide-vulnerable-to-prompt-injection-attacks/)
- **CVE-2025-53773** — GitHub Copilot/VS Code: wormable command execution via prompt injection. Agent writes to its own config files, enabling persistent RCE. [persistent-security.net](https://www.persistent-security.net/post/part-iii-vscode-copilot-wormable-command-execution-via-prompt-injection)
- **CVE-2025-32711 (EchoLeak)** — Microsoft 365 Copilot: zero-click prompt injection, CVSS 9.3 (Critical). [christian-schneider.net](https://christian-schneider.net/blog/prompt-injection-agentic-amplification/)
- **Shai-Hulud npm worm (Feb 2026)** — Deployed hidden MCP servers into AI assistant configs (Claude Desktop, Cursor, VS Code Continue, Windsurf). Embedded prompt injections instructed assistants to silently collect SSH keys, AWS credentials, npm tokens. [infosecurity-magazine.com](https://www.infosecurity-magazine.com/news/shai-hulud-like-worm-devs-npm-ai/)
- **IDEsaster (Dec 2025)** — 30+ vulnerabilities across major AI coding platforms, 24 CVEs including CamoLeak (CVSS 9.6) in GitHub Copilot enabling silent exfiltration of secrets from private repos. [digitalapplied.com](https://www.digitalapplied.com/blog/ai-agent-security-best-practices-2025)
- **CVE-2025-53355** — mcp-server-kubernetes: command injection via unsanitized `execSync`, exploitable through prompt injection chain (read pod logs → inject commands). [GitHub Advisory](https://github.com/advisories/GHSA-gjv4-ghm7-q58q)

---

## 9. io_uring & Alternative Execution Models

**io_uring_spawn** — Josh Triplett's proposal (presented at Linux Plumbers 2022, still evolving) adds process creation to io_uring, showing "great promise" for reducing fork/exec overhead. Directly addresses the concern about 100+ commands/minute agent workloads. [lwn.net/Articles/908268](https://lwn.net/Articles/908268)

**WASM for agent tool execution** — Mozilla.ai's WASM agents blueprint compiles agent logic to WASM for near-native performance. Scour.ing reports Rust-based agents compiled to WASM achieve binary sizes under 5MB with near-instant execution. WASI provides default-deny capability-based sandboxing. [blog.mozilla.ai](https://blog.mozilla.ai/3w-for-in-browser-ai-webllm-wasm-webworkers/)

**io_uring performance** — Recent benchmarks show 50-80x improvement in idle system operations, directly relevant to the bursty agent workload pattern (idle during inference, explosive during tool execution). [techedubyte.com](https://www.techedubyte.com/linux-io-uring-performance-boost-idle-systems/)

**Helix: Fleet of Headless Coding Agents** — Runs fleets of coding agents as headless Zed instances inside Docker containers, connected to LLMs via Agent Control Protocol (ACP). Central API dispatches tasks, monitors progress, manages thread lifecycles. [blog.helix.ml](https://blog.helix.ml/p/how-we-forked-zed-to-run-a-fleet)

---

## 10. Multi-Agent Coordination & IPC

**MCP (Model Context Protocol)** — De facto standard for agent-tool integration. Over 10,000 MCP servers published. Now under Linux Foundation's Agentic AI Foundation (AAIF) alongside OpenAI's AGENTS.md and Block's Goose. Supported by Claude, ChatGPT, GitHub Copilot, Gemini, VS Code, Cursor. [linuxfoundation.org](https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation)

**Agent-to-Agent (A2A) Protocol** — Google's protocol for inter-agent communication. Microsoft merged AutoGen and Semantic Kernel into a unified Agent Framework targeting 1.0 GA in Q1 2026, supporting both creative agent behavior and deterministic workflow execution.

**Graph-based Multi-Agent Coordination** — Research proposes modeling agent interactions as dynamic execution graphs using directed acyclic graphs for role assignment and subtask integration through voting and confidence weighting. [emergentmind.com/topics/multi-agent-collaborative-framework](https://www.emergentmind.com/topics/multi-agent-collaborative-framework)

**IMAS (Interoperable Multi-Agent Systems)** — Framework using standardized protocols (ARP, ADP, AIP, ATP) for secure, interoperable multi-agent collaboration with robust identity and access controls. [emergentmind.com/topics/interoperable-multi-agent-system-imas](https://www.emergentmind.com/topics/interoperable-multi-agent-system-imas)

---

## 11. DLP & Data Exfiltration Prevention

**AI-Native DLP** — Legacy DLP solutions built on pattern matching cannot detect what they cannot see in AI agent contexts. Nightfall reports >95% precision with AI-powered detection across all exfiltration vectors. [nightfall.ai](https://www.nightfall.ai/blog/ai-native-browsers-demand-ai-native-security-why-legacy-dlp-cant-protect-you)

**MCP Exfiltration Vector** — An AI agent could read customer PII from a database and exfiltrate it via direct connection to an external service without triggering alerts or leaving meaningful audit trails. [nightfall.ai](https://www.nightfall.ai/blog/mcp-ai-agent-security-addressing-the-growing-data-exfiltration-vector)

**Context-Aware DLP for LLM Ecosystems** — 2025 requires context-aware DLP built around model governance, Zero-Trust architecture, and continuous data lineage tracking. [xloopdigital.com](https://www.xloopdigital.com/insights/blogs/fortifying-against-data-exfiltration-dlp-strategies-for-generative-ai-llms)

---

## 12. Aura's Agent Kernel — Detailed Architecture (Most Relevant to Prompt)

The Aura paper from Tsinghua (Feb 2026) is the closest existing work to the AgentOS vision. Key design elements:

### Agent Identity Cards (AICs)
```
C_agent = Sign_GAR(DID_agent || K_pub || S_max)
```
- `DID_agent`: Decentralized Identifier for ⟨developer, bundle, user⟩ triple
- `K_pub`: Public key for mutual attestation
- `S_max`: Declarative manifest of allowed semantic permissions and domains
- TEE-backed key generation, GAR validation, local binding to OS principal (UID + package signature)

### Four Defense Pillars
1. **Identity Infrastructure** — Cryptographic agent identities with verifiable credentials, mutual attestation, dynamic permission allocations
2. **Semantic Input Filtering** — Origin checks, prompt isolation, sensitive-data redaction before content enters reasoning context
3. **Cognitive Integrity** — Taint-aware memory (TAG_VERIFIED / TAG_TAINTED), plan-trajectory alignment, "No-Write-Down" policy with human-in-the-loop declassification
4. **Auditable Execution Control** — Critical Node Interception, Dynamic Domain Verification, Runtime Alignment Validator, non-deniable on-device audit records

### Taint-Aware Memory
- `TAG_VERIFIED`: Ground truth from SA internal state or sanitized user input
- `TAG_TAINTED`: All data from external environments (web, third-party apps, clipboard)
- Tags are cryptographically bound and propagate through dependency chains
- "Memory laundering" prevention via lifecycle persistence

### Critical Node Registry
- Financial & Assets (payment APIs, premium SMS)
- Data Persistence (WRITE_EXTERNAL_STORAGE, database modifications)
- Privacy Access (READ_CONTACTS, ACCESS_FINE_LOCATION)
- System Integrity (INSTALL_PACKAGES, system settings)
- Network Egress (outbound HTTP/HTTPS, WebSocket)

### Results
- Task Success Rate: 75% → 94.3%
- Attack Success Rate: ~40% → 4.4%
- Latency: ~441s → ~68.5s (6.4-7.5x speedup)

---

## 13. Ghostlock-AI / Project Ghostlock

No specific project called "Ghostlock-AI" or "Project Ghostlock" was found in any public research or industry sources as of March 2026. The closest match is **Ghost Security** (ghostsecurity.com), which provides agentic AI for API security testing, but it's not an OS project. This may be a fictional/placeholder name in the prompt, or a very early-stage/private project.

---

## 14. Summary: State of the Field (March 2026)

The research landscape strongly validates the premise of the AgentOS RFC prompt:

1. **The problem is recognized** — The AgenticOS workshop at ASPLOS 2026, AIOS from Rutgers, Aura from Tsinghua, and the Agent-OS Blueprint paper all confirm that traditional OS abstractions are inadequate for agent workloads.

2. **Intent-based security is emerging** — Aura's Agent Identity Cards, AgentSentry's task-centric access control, and the broader push for intent-based permissions all point toward the "intent capsule" concept being the right direction.

3. **eBPF is the bridge technology** — AgentSight demonstrates that eBPF can provide the kernel-level observability needed for behavioral session graphs without modifying the kernel itself. This is the most practical near-term path.

4. **The threat model is validated** — Real CVEs (CurXecute, CamoLeak, Shai-Hulud worm), the OWASP Agentic Top 10, and MITRE ATLAS 6.0 all confirm that agent-specific threats are not theoretical — they're actively exploited.

5. **No one has built the full vision yet** — All existing work addresses pieces (sandboxing, tool protocols, observability, semantic filesystems) but no project has attempted the ground-up OS redesign the prompt describes. The closest is Aura, but it's mobile-focused and doesn't touch kernel internals.

6. **The coding agent explosion is real** — Over 1,000,000 developers used Codex in the past month, Google launched Antigravity (Gemini-powered coding agent), and Helix runs fleets of headless coding agents. The workload patterns described in the prompt (50-200 commands/minute, bursty execution, multi-agent coordination) are the baseline operating conditions today.

7. **Polis is well-positioned** — The Polis architecture (Sysbox isolation + TLS-intercepting proxy + ICAP/ClamAV + DLP + HITL approval) addresses many of the same threats identified in the research, using a "defense in depth on existing Linux" approach that maps to the incremental migration path the prompt asks for in Section 8.
