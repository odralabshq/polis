#!/bin/bash
# setup-vm.sh - Automated Polis VM Setup and SSH Authorization

set -e

NAME="polis-dev"
CPUS="4"
MEMORY="8G"
DISK="50G"

log_step() { echo -e "\n\033[0;36m[STEP] $1\033[0m"; }
log_info() { echo -e "\033[0;34m[INFO] $1\033[0m"; }
log_success() { echo -e "\033[0;32m[OK] $1\033[0m"; }

# 1. Check Multipass
if ! command -v multipass &> /dev/null; then
    echo "Multipass is not installed. Please visit https://multipass.run/install"
    exit 1
fi

# 2. Check/Generate SSH Key
SSH_DIR="$HOME/.ssh"
PUB_KEY_FILE="$SSH_DIR/id_ed25519.pub"
if [ ! -f "$PUB_KEY_FILE" ]; then
    PUB_KEY_FILE="$SSH_DIR/id_rsa.pub"
fi

if [ ! -f "$PUB_KEY_FILE" ]; then
    log_step "Generating SSH Key..."
    mkdir -p "$SSH_DIR"
    ssh-keygen -t ed25519 -f "$SSH_DIR/id_ed25519" -N ""
    PUB_KEY_FILE="$SSH_DIR/id_ed25519.pub"
    log_success "Generated new SSH key: $PUB_KEY_FILE"
else
    log_info "Using existing SSH key: $PUB_KEY_FILE"
fi

PUB_KEY=$(cat "$PUB_KEY_FILE")

# 3. Create VM
log_step "Creating VM '$NAME'..."
if multipass list | grep -q "^$NAME\s"; then
    log_info "VM '$NAME' already exists. Skipping creation."
else
    multipass launch \
        --name "$NAME" \
        --cpus "$CPUS" \
        --memory "$MEMORY" \
        --disk "$DISK" \
        --cloud-init polis-dev.yaml \
        24.04
    log_success "VM launched."
fi

# 4. Wait for Cloud-Init
log_step "Waiting for VM to be ready..."
multipass exec "$NAME" -- cloud-init status --wait
log_success "VM is ready."

# 5. Authorize Key
log_step "Authorizing your host's SSH key..."
multipass exec "$NAME" -- bash -c "mkdir -p ~/.ssh && echo '$PUB_KEY' >> ~/.ssh/authorized_keys"
log_success "Key authorized."

# 6. Get IP and Show Config
IP=$(multipass info "$NAME" --format json | jq -r ".info.\"$NAME\".ipv4[0]")

echo -e "\n\033[1;33m====================================================\033[0m"
echo -e "\033[1;32mSetup Complete!\033[0m"
echo -e "\033[1;33m====================================================\033[0m"

echo -e "\n\033[0;36mSSH CONFIGURATION FOR VS CODE:\033[0m"
echo "-------------------------------"
echo "Host $NAME"
echo "    HostName $IP"
echo "    User ubuntu"
echo "    IdentityFile ${PUB_KEY_FILE%.pub}"
echo "-------------------------------"

echo -e "\nNext Steps:"
echo "1. Open VS Code."
echo "2. Connect to Host '$NAME' (Remote-SSH)."
echo "3. Open folder '~/polis'."
echo "4. Inside the terminal, run: ./cli/polis.sh init --local"
