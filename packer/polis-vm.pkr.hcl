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
    hyperv = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/hyperv"
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

variable "ubuntu_iso_url" {
  type        = string
  description = "URL to Ubuntu 24.04 Mini ISO"
  default     = "https://cdimage.ubuntu.com/ubuntu-mini-iso/noble/daily-live/current/noble-mini-iso-amd64.iso"
}

variable "ubuntu_iso_checksum" {
  type        = string
  description = "SHA256 of the Ubuntu 24.04 Mini ISO"
  default     = "077ff0e8079eae284a50d18673bb11030bfa4bbddc4f4e446e84e3a7b42b485c"
}

variable "hyperv_switch_name" {
  type        = string
  description = "Hyper-V virtual switch name"
  default     = "Default Switch"
}

variable "hyperv_generation" {
  type    = number
  default = 2
}

variable "headless" {
  type        = bool
  description = "Run QEMU/Hyper-V headless (set false to open console for debugging)"
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
  sysbox_sha256 = var.arch == "amd64" ? var.sysbox_sha256_amd64 : var.sysbox_sha256_arm64
  # Random password for build-time SSH — never stored in final image (passwd -l)
  build_password = uuidv4()
}

# ============================================================================
# Source
# ============================================================================

source "qemu" "polis" {
  iso_url          = var.ubuntu_iso_url
  iso_checksum     = var.ubuntu_iso_checksum
  disk_image       = false
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
  ssh_password     = local.build_password
  ssh_timeout      = "40m"

  cd_content = {
    "meta-data" = ""
    "user-data" = <<-EOF
      #cloud-config
      autoinstall:
        version: 1
        install-type:
          minimal: true
        identity:
          hostname: polis-vm
          password: "${local.build_password}"
          username: ubuntu
        ssh:
          install-server: true
          allow-pw: true
        user-data:
          users:
            - name: ubuntu
              sudo: ALL=(ALL) NOPASSWD:ALL
              shell: /bin/bash
              lock_passwd: false
        late-commands:
          - "apt-get purge -y snapd"
          - "rm -rf /var/cache/snapd"
          - "apt-get autoremove -y"
    EOF
  }
  cd_label = "CIDATA"

  boot_wait = "5s"
  boot_command = [
    "<wait>c<wait>",
    "set linux_gfx_mode=800x600<enter>",
    "linux /casper/vmlinuz quiet autoinstall ds=nocloud;s=/cdrom/ ---<enter>",
    "initrd /casper/initrd<enter>",
    "boot<enter>"
  ]
}

source "hyperv-iso" "polis" {
  iso_url               = var.ubuntu_iso_url
  iso_checksum          = var.ubuntu_iso_checksum
  output_directory      = "output-hyperv"
  vm_name               = "polis-${var.polis_version}-${var.arch}.vhdx"
  switch_name           = var.hyperv_switch_name
  generation            = var.hyperv_generation
  enable_dynamic_memory = true
  disk_size             = "20480"
  memory                = 4096
  cpus                  = 2
  headless              = var.headless
  shutdown_command      = "sudo shutdown -P now"
  ssh_username          = "ubuntu"
  ssh_password          = local.build_password
  ssh_timeout           = "40m"

  # Autoinstall config for fresh ISO install
  cd_content = {
    "meta-data" = ""
    "user-data" = <<-EOF
      #cloud-config
      autoinstall:
        version: 1
        install-type:
          minimal: true
        identity:
          hostname: polis-vm
          password: "${local.build_password}"
          username: ubuntu
        ssh:
          install-server: true
          allow-pw: true
        user-data:
          users:
            - name: ubuntu
              sudo: ALL=(ALL) NOPASSWD:ALL
              shell: /bin/bash
              lock_passwd: false
        late-commands:
          - "apt-get purge -y snapd"
          - "rm -rf /var/cache/snapd"
          - "apt-get autoremove -y"
    EOF
  }
  cd_label = "CIDATA"

  boot_wait = "5s"
  boot_command = [
    "<wait>c<wait>",
    "set linux_gfx_mode=800x600<enter>",
    "linux /casper/vmlinuz quiet autoinstall ds=nocloud;s=/cdrom/ ---<enter>",
    "initrd /casper/initrd<enter>",
    "boot<enter>"
  ]
}

# ============================================================================
# Build
# ============================================================================

build {
  sources = [
    "source.qemu.polis",
    "source.hyperv-iso.polis"
  ]

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

  # Cleanup and reclaim space
  provisioner "shell" {
    inline = [
      "sudo fstrim -av || true",
      "sudo dd if=/dev/zero of=/EMPTY bs=1M 2>/dev/null || true",
      "sudo rm -f /EMPTY",
      "sudo sync",
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*",
      # Full cloud-init reset so the image works on any hypervisor (QEMU, Hyper-V, etc.)
      # --logs: remove /var/log/cloud-init*
      # --machine-id: truncate /etc/machine-id so systemd regenerates it
      # --configs network: remove generated netplan/network configs so cloud-init
      #   re-detects the NIC on the target hypervisor (QEMU virtio vs Hyper-V hv_netvsc)
      "sudo cloud-init clean --logs --machine-id --configs network",
      "sudo passwd -l ubuntu",
      "sudo passwd -l root"
    ]
  }
}
