# polis-vm.pkr.hcl - Packer template for Polis VM image
# Builds a hardened Ubuntu 24.04 VM with Docker + Sysbox pre-installed

packer {
  required_plugins {
    qemu = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/qemu"
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

variable "arch" {
  type    = string
  default = "amd64"
}

variable "ubuntu_serial" {
  type        = string
  description = "Ubuntu cloud image release serial for reproducible builds"
  default     = "20250115"
}

# ============================================================================
# Locals
# ============================================================================

locals {
  ubuntu_url      = "https://cloud-images.ubuntu.com/releases/24.04/release-${var.ubuntu_serial}/ubuntu-24.04-server-cloudimg-${var.arch}.img"
  ubuntu_checksum = "file:https://cloud-images.ubuntu.com/releases/24.04/release-${var.ubuntu_serial}/SHA256SUMS"
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
  vm_name          = "polis-vm-${var.polis_version}-${var.arch}.qcow2"
  format           = "qcow2"
  disk_size        = "20G"
  memory           = 4096
  cpus             = 2
  headless         = true
  shutdown_command = "sudo shutdown -P now"
  ssh_username     = "ubuntu"
  ssh_timeout      = "10m"

  # Cloud-init injects Packer's ephemeral SSH public key (no hardcoded credentials)
  cd_content = {
    "meta-data" = ""
    "user-data" = <<-EOF
      #cloud-config
      users:
        - name: ubuntu
          sudo: ALL=(ALL) NOPASSWD:ALL
          shell: /bin/bash
          lock_passwd: true
          ssh_authorized_keys:
            - ${build.SSHPublicKey}
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

  # Cleanup
  provisioner "shell" {
    inline = [
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*",
      "sudo cloud-init clean --logs"
    ]
  }
}
