#!/bin/bash
set -euo pipefail

echo "[workspace] Starting initialization..."

# Update CA certificates — ensure Polis CA is in the system bundle.
# The CA cert is bind-mounted read-only with non-root ownership, which
# prevents update-ca-certificates from auto-detecting it on some Debian
# versions. We copy it to /usr/share/ca-certificates/ (writable) first.
if [[ -f /usr/local/share/ca-certificates/polis-ca.crt ]]; then
    cp /usr/local/share/ca-certificates/polis-ca.crt \
       /usr/share/ca-certificates/polis-ca.crt 2>/dev/null || true
    grep -qxF 'polis-ca.crt' /etc/ca-certificates.conf 2>/dev/null || \
        echo 'polis-ca.crt' >> /etc/ca-certificates.conf
fi
update-ca-certificates 2>/dev/null || true
# Fallback: append directly if update-ca-certificates didn't include it.
if [[ -f /usr/local/share/ca-certificates/polis-ca.crt ]] && \
   ! grep -q "Polis CA" /etc/ssl/certs/ca-certificates.crt 2>/dev/null; then
    cat /usr/local/share/ca-certificates/polis-ca.crt >> /etc/ssl/certs/ca-certificates.crt
fi

# =============================================================================
# SSL/TLS CA Trust — make ALL runtimes trust the Polis CA.
#
# The transparent proxy (g3proxy) terminates TLS and re-signs with the Polis
# CA. Different languages use different trust stores:
#
#   System CA store (/etc/ssl/certs/)  → Go, .NET, curl, wget, git, apt, dpkg
#   SSL_CERT_FILE env var              → OpenSSL-linked tools: Ruby, Perl, C
#   REQUESTS_CA_BUNDLE env var         → Python requests, pip, httpx
#   CURL_CA_BUNDLE env var             → curl, libcurl-based tools
#   NODE_EXTRA_CA_CERTS env var        → Node.js (https, fetch, npm)
#   HTTPS_CA_FILE env var              → Perl LWP::UserAgent
#   GIT_SSL_CAINFO env var             → git (fallback if system store fails)
#   openssl.cafile / curl.cainfo       → PHP (via php.ini)
#   Java cacerts keystore              → JVM (keytool import required)
#   Python certifi-system-store        → Patches certifi to use system store
#
# update-ca-certificates (above) handles the system store. Everything below
# handles the per-runtime overrides.
# =============================================================================
CA_BUNDLE="/etc/ssl/certs/ca-certificates.crt"
CA_CERT="/usr/local/share/ca-certificates/polis-ca.crt"

# --- Environment variables (covers ~80% of tools) ---
# /etc/environment is read by PAM/systemd for all login sessions.
# /etc/profile.d/ covers interactive shells (SSH, agent terminals).
ENV_VARS=(
    "REQUESTS_CA_BUNDLE=${CA_BUNDLE}"   # Python requests, pip, httpx
    "SSL_CERT_FILE=${CA_BUNDLE}"        # OpenSSL: Ruby, Perl, generic
    "SSL_CERT_DIR=/etc/ssl/certs"       # OpenSSL: directory-based lookup
    "CURL_CA_BUNDLE=${CA_BUNDLE}"       # curl, libcurl
    "NODE_EXTRA_CA_CERTS=${CA_BUNDLE}"  # Node.js
    "HTTPS_CA_FILE=${CA_BUNDLE}"        # Perl LWP::UserAgent
    "GIT_SSL_CAINFO=${CA_BUNDLE}"       # git (explicit override)
)

for var in "${ENV_VARS[@]}"; do
    echo "$var" >> /etc/environment
done

{
    echo '# Polis CA trust — auto-generated, do not edit'
    for var in "${ENV_VARS[@]}"; do
        echo "export ${var}"
    done
} > /etc/profile.d/polis-ca.sh
chmod 644 /etc/profile.d/polis-ca.sh

# Export into the current init process so agent install scripts inherit them.
for var in "${ENV_VARS[@]}"; do
    export "${var?}"
done

# --- Java: import Polis CA into every JVM's cacerts keystore ---
# Java uses its own trust store (PKCS12/JKS), completely ignoring the OS.
# We must use keytool to inject the CA into each installed JVM.
import_java_ca() {
    local ca_cert="$1"
    local imported=0

    # Find all cacerts files across all Java installations
    for cacerts in \
        /usr/lib/jvm/*/lib/security/cacerts \
        /usr/lib/jvm/*/jre/lib/security/cacerts \
        /usr/java/*/lib/security/cacerts \
        /usr/java/*/jre/lib/security/cacerts \
        /opt/java/*/lib/security/cacerts; do
        [[ -f "$cacerts" ]] || continue

        # Find keytool in the same JVM
        local jvm_dir
        jvm_dir=$(echo "$cacerts" | sed 's|/lib/security/cacerts||;s|/jre/lib/security/cacerts||')
        local keytool="${jvm_dir}/bin/keytool"
        [[ -x "$keytool" ]] || keytool=$(command -v keytool 2>/dev/null || true)
        [[ -n "$keytool" ]] || continue

        # Skip if already imported
        if "$keytool" -list -keystore "$cacerts" -storepass changeit \
                -alias polis-ca &>/dev/null; then
            echo "[workspace] Java: Polis CA already in ${cacerts}"
            imported=$((imported + 1))
            continue
        fi

        echo "[workspace] Java: importing Polis CA into ${cacerts}..."
        if "$keytool" -importcert -noprompt -trustcacerts \
                -alias polis-ca \
                -file "$ca_cert" \
                -keystore "$cacerts" \
                -storepass changeit 2>/dev/null; then
            echo "[workspace] Java: imported into ${cacerts}"
            imported=$((imported + 1))
        else
            echo "[workspace] Java: WARNING — failed to import into ${cacerts}"
        fi
    done

    # Also set JAVA_TOOL_OPTIONS as a fallback for JVMs we didn't find.
    # javax.net.ssl.trustStoreType=PKCS12 is the default since Java 9+.
    if [[ $imported -eq 0 ]] && command -v java &>/dev/null; then
        echo "[workspace] Java: no cacerts found, setting JAVA_TOOL_OPTIONS fallback"
        echo "JAVA_TOOL_OPTIONS=-Djavax.net.ssl.trustStore=${CA_BUNDLE}" >> /etc/environment
        echo "export JAVA_TOOL_OPTIONS=\"-Djavax.net.ssl.trustStore=${CA_BUNDLE}\"" \
            >> /etc/profile.d/polis-ca.sh
    fi

    return 0
}

if [[ -f "$CA_CERT" ]]; then
    import_java_ca "$CA_CERT"
else
    echo "[workspace] CA trust: no Polis CA cert found, skipping Java import"
fi

# --- PHP: configure openssl.cafile and curl.cainfo ---
# PHP uses its own OpenSSL config (php.ini), not the env vars.
for ini_dir in /etc/php/*/cli/conf.d /etc/php/*/fpm/conf.d /etc/php/*/apache2/conf.d; do
    [[ -d "$ini_dir" ]] || continue
    cat > "${ini_dir}/99-polis-ca.ini" <<PHPEOF
; Polis CA trust — auto-generated
openssl.cafile=${CA_BUNDLE}
curl.cainfo=${CA_BUNDLE}
PHPEOF
    echo "[workspace] PHP: configured ${ini_dir}/99-polis-ca.ini"
done
# Fallback: if no versioned dirs exist, try the main php.ini locations
for ini_file in /etc/php.ini /usr/local/etc/php/php.ini; do
    [[ -f "$ini_file" ]] || continue
    if ! grep -q "polis-ca" "$ini_file" 2>/dev/null; then
        {
            echo ""
            echo "; Polis CA trust — auto-generated"
            echo "openssl.cafile=${CA_BUNDLE}"
            echo "curl.cainfo=${CA_BUNDLE}"
        } >> "$ini_file"
        echo "[workspace] PHP: appended CA config to ${ini_file}"
    fi
done

# --- Python: install certifi-system-store if certifi is present ---
# certifi bundles Mozilla's CAs and ignores the system store. The
# REQUESTS_CA_BUNDLE env var overrides it, but certifi-system-store
# patches certifi.where() itself so even code that reads the path
# directly (without checking env vars) gets the system bundle.
if command -v pip3 &>/dev/null; then
    if pip3 show certifi &>/dev/null 2>&1; then
        if ! pip3 show certifi-system-store &>/dev/null 2>&1; then
            echo "[workspace] Python: installing certifi-system-store..."
            pip3 install --quiet --break-system-packages \
                certifi-system-store 2>/dev/null || \
            pip3 install --quiet certifi-system-store 2>/dev/null || \
                echo "[workspace] Python: WARNING — certifi-system-store install failed"
        fi
    fi
fi

echo "[workspace] CA trust configuration complete"

# Source shared network helpers
SCRIPT_DIR="$(dirname "$0")"
if [[ -f "$SCRIPT_DIR/network-helpers.sh" ]]; then
    source "$SCRIPT_DIR/network-helpers.sh"
elif [[ -f "/usr/local/bin/network-helpers.sh" ]]; then
    source "/usr/local/bin/network-helpers.sh"
fi


if ! type disable_ipv6 &>/dev/null; then
    # Disable IPv6 at kernel level (Sysbox virtualizes procfs, so this works without --privileged)
    # SECURITY: Fail-closed - abort if IPv6 cannot be verified disabled
    disable_ipv6() {
        local container="${1:-workspace}"
        echo "[$container] Disabling IPv6..."
        
        # Native Linux + Sysbox: Disable via sysctl (procfs is virtualized per container)
        if sysctl -w net.ipv6.conf.all.disable_ipv6=1 >/dev/null 2>&1 && \
           sysctl -w net.ipv6.conf.default.disable_ipv6=1 >/dev/null 2>&1; then
            echo "[$container] IPv6 disabled via sysctl"
        else
            echo "[$container] WARNING: sysctl IPv6 disable failed"
        fi
        
        # FAIL-CLOSED: Verify no IPv6 addresses exist at all
        if ip -6 addr show 2>/dev/null | grep -q "inet6"; then
            echo "[$container] CRITICAL: IPv6 addresses still present after disable attempt:"
            ip -6 addr show 2>/dev/null || true
            echo "[$container] Aborting - TPROXY bypass risk"
            return 1
        fi
        
        echo "[$container] IPv6 verified disabled"
        return 0
    }
fi

# Protect sensitive paths — defense-in-depth layer (secondary to tmpfs mounts)
# chmod 000 existing dirs, create decoys for missing ones
protect_sensitive_paths() {
    local paths=(".ssh" ".aws" ".gnupg" ".config/gcloud" ".kube" ".docker")
    local home_dir="${HOME:-/root}"

    echo "[workspace] Protecting sensitive paths..."
    for p in "${paths[@]}"; do
        local full_path="$home_dir/$p"
        if [[ -d "$full_path" ]]; then
            chmod 000 "$full_path"
            echo "[workspace] Protected existing: $full_path"
        else
            mkdir -p "$full_path"
            chmod 000 "$full_path"
            echo "[workspace] Created decoy: $full_path"
        fi
    done
    echo "[workspace] Sensitive paths protected (6 paths)"
}

disable_ipv6 "workspace" || exit 1

# Generate SSH host keys if missing and start sshd
if [[ ! -f /etc/ssh/ssh_host_ed25519_key ]]; then
    echo "[workspace] Generating SSH host keys..."
    ssh-keygen -A
fi
# Unlock polis account for pubkey auth (shadow '!' blocks auth even with PubkeyAuthentication yes)
usermod -p '*' polis 2>/dev/null || true
echo "[workspace] Starting SSH daemon..."
systemctl enable ssh
systemctl start ssh

# Configure default route to gate for TPROXY FIRST — the workspace is on an
# internal-only Docker network with no default gateway. Without this route,
# there is zero internet connectivity (install.sh, apt-get, curl all fail).
echo "[workspace] Resolving gate IP..."
GATE_IP=$(getent hosts gate | awk '{print $1}')

if [[ -z "$GATE_IP" ]]; then
    echo "[workspace] ERROR: Could not resolve 'gate' service" >&2
    exit 1
fi

echo "[workspace] Configuring default route via gate (${GATE_IP})..."

# Remove any existing default route
ip route del default 2>/dev/null || true

# Add default route through gate
if ip route add default via "$GATE_IP"; then
    echo "[workspace] Default route configured successfully"
    ip route show
else
    echo "[workspace] ERROR: Failed to configure default route" >&2
    exit 1
fi

# Bootstrap agents AFTER routing is configured so they have internet.
# Supports two modes:
#   1. Pre-installed (image-based): scripts already at /usr/local/bin, install.sh is a no-op
#   2. Mounted (legacy/fallback): agents bind-mounted at /opt/agents/*/
for agent_dir in /opt/agents/*/; do
    [[ -d "$agent_dir" ]] || continue
    name=$(basename "$agent_dir")
    echo "[workspace] Bootstrapping agent: ${name}"

    # Run install.sh in a subshell so failures don't kill workspace init.
    # Pre-installed images have /var/lib/openclaw-installed marker — install.sh exits immediately.
    if [[ -x "${agent_dir}/install.sh" ]]; then
        if ! ("${agent_dir}/install.sh"); then
            echo "[workspace] WARNING: ${name}/install.sh failed — agent may not work"
            continue
        fi
    fi

    # Service enablement is handled below (with integrity checks)

    # Symlink polis-* scripts into PATH so the agent can invoke them
    # directly without searching the filesystem (find / returns exit 1
    # due to permission-denied directories, confusing the agent).
    for script in "${agent_dir}"/scripts/polis-*.sh; do
        [[ -f "$script" ]] || continue
        base=$(basename "$script" .sh)
        ln -sf "$script" "/usr/local/bin/${base}"
    done
done

# For pre-installed agents (image-based), scripts are already at /usr/local/share/<agent>/scripts/.
# Symlink them into PATH if not already present (handles the case where /opt/agents is not mounted).
for scripts_dir in /usr/local/share/*/scripts/; do
    [[ -d "$scripts_dir" ]] || continue
    for script in "${scripts_dir}"/polis-*.sh; do
        [[ -f "$script" ]] || continue
        base=$(basename "$script" .sh)
        [[ -L "/usr/local/bin/${base}" ]] && continue  # already symlinked from mount
        ln -sf "$script" "/usr/local/bin/${base}"
    done
done

# Protect sensitive directories (defense-in-depth, secondary to tmpfs mounts)
protect_sensitive_paths

# Collect and start agent services (with integrity verification from manifest system).
# install.sh already ran above (before routing), so we only handle .service files here.
agent_services=()
for agent_dir in /opt/agents/*/; do
    [[ -d "$agent_dir" ]] || continue
    name=$(basename "$agent_dir")

    # Collect services to enable (generated .service file is mounted by compose override)
    svc="/etc/systemd/system/${name}.service"
    if [[ -f "$svc" ]]; then
        # Verify .service file integrity (hash generated at polis init time)
        hash_file="/etc/systemd/system/${name}.service.sha256"
        if [[ -f "$hash_file" ]]; then
            expected=$(cat "$hash_file")
            actual=$(sha256sum "$svc" | cut -d' ' -f1)
            if [[ "$expected" != "$actual" ]]; then
                echo "[workspace] CRITICAL: ${name}.service integrity check failed. Skipping."
                continue
            fi
            echo "[workspace] ${name}.service integrity verified"
        fi
        agent_services+=("${name}.service")
    fi
done

# Single daemon-reload, then enable and start all collected services.
# IMPORTANT: Use --no-block to avoid deadlock. Agent services declare
# Requires=polis-init.service, so they wait for this script to finish.
# Using "enable --now" (which blocks until the service is active) would
# deadlock: init waits for agent → agent waits for init.
if [[ ${#agent_services[@]} -gt 0 ]]; then
    systemctl daemon-reload
    for svc in "${agent_services[@]}"; do
        systemctl enable "$svc" || \
            echo "[workspace] WARNING: failed to enable ${svc}"
        systemctl start --no-block "$svc" || \
            echo "[workspace] WARNING: failed to queue start for ${svc}"
    done
fi

echo "[workspace] Initialization complete"
exit 0
