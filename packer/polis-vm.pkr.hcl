# polis-vm.pkr.hcl - Packer template for Polis VM image
# Builds a hardened Ubuntu 24.04 LTS Minimal VM with Docker + Sysbox pre-installed
#
# Distro Choice: Ubuntu 24.04 LTS (Noble Numbat) Minimal
# - Sysbox: First-class support with official .deb packages
# - eBPF: Kernel 6.8+ with full BTF/CO-RE support
# - AppArmor: Enabled by default
# - LTS: Supported until 2029 (standard), 2034 (ESM)
# - Minimal image: ~248MB base (vs ~2GB full server), reduced attack surface

packer {
  required_plugins {
    qemu = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/qemu"
    }
    goss = {
      version = ">= 3.2.0"
      source  = "github.com/YaleUniversity/goss"
    }
  }
}

# ============================================================================
# Variables
# ============================================================================

variable "polis_version" {
  type    = string
  default = "dev"
}

variable "sysbox_version" {
  type    = string
  default = "0.6.7"
}

variable "sysbox_sha256_amd64" {
  type        = string
  description = "SHA256 of the Sysbox amd64 .deb package"
  default     = "b7ac389e5a19592cadf16e0ca30e40919516128f6e1b7f99e1cb4ff64554172e"
}

variable "sysbox_sha256_arm64" {
  type        = string
  description = "SHA256 of the Sysbox arm64 .deb package"
  default     = "16d80123ba53058cf90f5a68686e297621ea97942602682e34b3352783908f91"
}

variable "images_tar" {
  type    = string
  default = ".build/polis-images.tar"
}

variable "config_tar" {
  type        = string
  description = "Path to polis-config.tar.gz bundle"
  default     = ".build/polis-config.tar.gz"
}

variable "agents_tar" {
  type        = string
  description = "Path to polis-agents.tar.gz bundle"
  default     = ""
}

variable "arch" {
  type    = string
  default = "amd64"
}

variable "ubuntu_serial" {
  type        = string
  description = "Ubuntu minimal cloud image release serial for reproducible builds"
  default     = "20260128"
}

variable "use_minimal_image" {
  type        = bool
  description = "Use Ubuntu Minimal cloud image (recommended: smaller footprint, reduced attack surface)"
  default     = true
}

variable "headless" {
  type        = bool
  description = "Run QEMU headless (set false to open console for debugging)"
  default     = true
}

variable "accelerator" {
  type        = string
  description = "QEMU accelerator: 'kvm' (native Linux, fast) or 'tcg' (inside VM without nested virt, slow)"
  default     = "kvm"
}

# ============================================================================
# Locals
# ============================================================================

locals {
  # Minimal image: ~248MB, reduced attack surface (recommended)
  # Full image: ~2GB, more packages pre-installed
  ubuntu_base_url = var.use_minimal_image ? "https://cloud-images.ubuntu.com/minimal/releases/noble/release" : "https://cloud-images.ubuntu.com/releases/24.04/release-${var.ubuntu_serial}"
  ubuntu_img_name = var.use_minimal_image ? "ubuntu-24.04-minimal-cloudimg-${var.arch}.img" : "ubuntu-24.04-server-cloudimg-${var.arch}.img"
  ubuntu_url      = "${local.ubuntu_base_url}/${local.ubuntu_img_name}"
  ubuntu_checksum = "file:${local.ubuntu_base_url}/SHA256SUMS"
  sysbox_sha256   = var.arch == "amd64" ? var.sysbox_sha256_amd64 : var.sysbox_sha256_arm64
}

# ============================================================================
# Source
# ============================================================================

source "qemu" "polis" {
  iso_url          = local.ubuntu_url
  iso_checksum     = local.ubuntu_checksum
  disk_image       = true
  output_directory = "output"
  vm_name          = "polis-${var.polis_version}-${var.arch}.qcow2"
  format           = "qcow2"
  disk_compression = true
  disk_size        = "20G"
  memory           = 4096
  cpus             = 2
  accelerator      = var.accelerator
  headless         = var.headless
  shutdown_command = "sudo shutdown -P now"
  ssh_username     = "ubuntu"
  ssh_password     = "ubuntu"
  ssh_timeout      = "20m"

  # Retry until cloud-init finishes enabling password auth
  ssh_handshake_attempts = 100

  # Cloud-init: create packer user with password auth enabled
  cd_content = {
    "meta-data" = ""
    "user-data" = <<-EOF
      #cloud-config
      users:
        - name: ubuntu
          sudo: ALL=(ALL) NOPASSWD:ALL
          shell: /bin/bash
          lock_passwd: false
      chpasswd:
        expire: false
        users:
          - name: ubuntu
            password: ubuntu
            type: text
      ssh_pwauth: true
    EOF
  }
  cd_label = "CIDATA"
}

# ============================================================================
# Build
# ============================================================================

build {
  sources = ["source.qemu.polis"]

  # Install Docker via apt repository with GPG verification
  provisioner "shell" {
    script = "scripts/install-docker.sh"
  }

  # Install Sysbox with SHA256 verification
  provisioner "shell" {
    script = "scripts/install-sysbox.sh"
    environment_vars = [
      "SYSBOX_VERSION=${var.sysbox_version}",
      "SYSBOX_SHA256=${local.sysbox_sha256}",
      "ARCH=${var.arch}"
    ]
  }

  # Upload pre-built images tarball
  provisioner "file" {
    source      = var.images_tar
    destination = "/tmp/polis-images.tar"
  }

  # Load Docker images
  provisioner "shell" {
    script = "scripts/load-images.sh"
  }

  # Apply VM hardening (sysctl, AppArmor, auditd, Docker daemon)
  provisioner "shell" {
    script = "scripts/harden-vm.sh"
  }

  # Upload Polis config bundle
  provisioner "file" {
    source      = var.config_tar
    destination = "/tmp/polis-config.tar.gz"
  }

  # Install Polis orchestration (compose, certs, systemd service)
  provisioner "shell" {
    script = "scripts/install-polis.sh"
  }

  # Upload agents tarball (if provided)
  provisioner "file" {
    source      = var.agents_tar
    destination = "/tmp/polis-agents.tar.gz"
  }

  # Install pre-generated agent artifacts
  provisioner "shell" {
    script = "scripts/install-agents.sh"
  }

  # Validate VM image with Goss tests before finalizing
  provisioner "goss" {
    tests = [
      "goss/goss.yaml",
      "goss/goss-docker.yaml",
      "goss/goss-sysbox.yaml",
      "goss/goss-hardening.yaml",
      "goss/goss-polis.yaml"
    ]
    vars_file = "goss/goss-vars.yaml"
    vars_env = {
      SYSBOX_VERSION = var.sysbox_version
    }
    retry_timeout = "30s"
    sleep         = "2s"
  }

  # Cleanup
  provisioner "shell" {
    inline = [
      "sudo dd if=/dev/zero of=/EMPTY bs=1M 2>/dev/null || true",
      "sudo rm -f /EMPTY",
      "sudo sync",
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*",
      "sudo cloud-init clean --logs",
      "sudo passwd -l ubuntu",
      "sudo passwd -l root"
    ]
  }
}
