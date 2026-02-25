# polis-vm.pkr.hcl - Packer template for Polis VM image
# Builds Ubuntu 24.04 LTS VM with Docker + Sysbox
#
# ISO: ubuntu-24.04.4-live-server-amd64.iso
#   The "mini ISO" is a network version-chooser, not an installer — it cannot
#   do unattended autoinstall. The live-server ISO is used instead, with
#   autoinstall configured to install only the minimal required packages.
#
# Hyper-V notes:
#   - Generation 2 + UEFI: Secure Boot must be disabled for unsigned Ubuntu ISO
#   - IP discovery: Packer uses Hyper-V KVP; requires hyperv-daemons in the guest
#   - Boot: live-server GRUB menu appears ~30s after UEFI POST on Default Switch

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
  type    = string
  default = "b7ac389e5a19592cadf16e0ca30e40919516128f6e1b7f99e1cb4ff64554172e"
}

variable "sysbox_sha256_arm64" {
  type    = string
  default = "16d80123ba53058cf90f5a68686e297621ea97942602682e34b3352783908f91"
}

variable "images_tar" {
  type    = string
  default = ".build/polis-images.tar"
}

variable "config_tar" {
  type    = string
  default = ".build/polis-config.tar.gz"
}

variable "agents_tar" {
  type    = string
  default = ""
}

variable "arch" {
  type    = string
  default = "amd64"
}

variable "ubuntu_iso_url" {
  type    = string
  default = "https://releases.ubuntu.com/noble/ubuntu-24.04.4-live-server-amd64.iso"
}

variable "ubuntu_iso_checksum" {
  type    = string
  default = "sha256:e907d92eeec9df64163a7e454cbc8d7755e8ddc7ed42f99dbc80c40f1a138433"
}

variable "hyperv_switch_name" {
  type    = string
  default = "Default Switch"
}

variable "hyperv_generation" {
  type    = number
  default = 2
}

variable "headless" {
  type    = bool
  default = true
}

variable "accelerator" {
  type    = string
  default = "kvm"
}

# ============================================================================
# Locals
# ============================================================================

locals {
  sysbox_sha256  = var.arch == "amd64" ? var.sysbox_sha256_amd64 : var.sysbox_sha256_arm64
  build_password = "packer-build-only"
}

# ============================================================================
# Autoinstall user-data (shared between QEMU and Hyper-V)
# Installs: openssh-server + linux-cloud-tools-virtual (Hyper-V IP reporting)
# Everything else (Docker, Sysbox, etc.) is installed by provisioners.
# Based on marcinbojko/hv-packer Ubuntu 24.04 template.
# ============================================================================

locals {
  autoinstall_user_data = <<-EOF
    #cloud-config
    autoinstall:
      version: 1
      locale: en_US.UTF-8
      keyboard:
        layout: us
      early-commands:
        - systemctl stop ssh
      update: no
      network:
        network:
          version: 2
          ethernets:
            eth0:
              dhcp4: yes
              dhcp-identifier: mac
      apt:
        geoip: false
        preserve_sources_list: false
        primary:
          - arches: [amd64]
            uri: "http://archive.ubuntu.com/ubuntu/"
      storage:
        layout:
          name: direct
      identity:
        hostname: polis-vm
        username: ubuntu
        password: "${local.build_password}"
      ssh:
        install-server: true
        allow-pw: true
      packages:
        - linux-cloud-tools-virtual
        - linux-tools-virtual
      user-data:
        disable_root: false
        ssh_pwauth: true
        chpasswd:
          expire: false
          users:
            - name: ubuntu
              password: "${local.build_password}"
              type: text
        users:
          - name: ubuntu
            sudo: ALL=(ALL) NOPASSWD:ALL
            shell: /bin/bash
            lock_passwd: false
      late-commands:
        - "curtin in-target -- systemctl enable ssh.service"
        - "curtin in-target -- systemctl disable ssh.socket"
  EOF
}

# ============================================================================
# Source: QEMU (Linux CI)
# ============================================================================

source "qemu" "polis" {
  iso_url          = var.ubuntu_iso_url
  iso_checksum     = var.ubuntu_iso_checksum
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
  ssh_timeout      = "60m"

  cd_content = {
    "meta-data" = ""
    "user-data" = local.autoinstall_user_data
  }
  cd_label = "CIDATA"

  # live-server ISO GRUB: press 'e', go to linux line, append autoinstall params
  boot_wait = "5s"
  boot_command = [
    "<wait5>e<wait2>",
    "<down><down><end>",
    " autoinstall ds=nocloud\\;s=/cdrom/",
    "<f10>"
  ]
}

# ============================================================================
# Source: Hyper-V (Windows build)
# ============================================================================

source "hyperv-iso" "polis" {
  iso_url          = var.ubuntu_iso_url
  iso_checksum     = var.ubuntu_iso_checksum
  output_directory = "output-hyperv"
  vm_name          = "polis-${var.polis_version}-${var.arch}"
  switch_name      = var.hyperv_switch_name
  generation       = var.hyperv_generation

  # Skip disk compaction — Packer's Optimize-VHD fails on temp path after VM unregister
  skip_compaction  = true

  # Secure Boot blocks unsigned Ubuntu ISO on Gen 2
  enable_secure_boot = false

  # Nested virt required for Sysbox/Docker-in-Docker
  enable_virtualization_extensions = true
  enable_mac_spoofing              = true   # required with virtualization extensions
  enable_dynamic_memory            = false  # must be false with virtualization extensions

  disk_size        = "10240"
  memory           = 4096
  cpus             = 2
  headless         = var.headless
  shutdown_command = "sudo shutdown -P now"
  ssh_username     = "ubuntu"
  ssh_password     = local.build_password
  ssh_timeout      = "60m"

  # Serve autoinstall over HTTP so the installer can fetch it during boot.
  # cd_content (CIDATA ISO) is unreliable on Hyper-V Gen 2 — the secondary
  # DVD is not always visible to the installer before network comes up.
  http_content = {
    "/meta-data" = ""
    "/user-data" = local.autoinstall_user_data
  }

  # GRUB command-line approach (proven pattern from marcinbojko/hv-packer).
  # Gen 2 UEFI POST is fast (~2-3s). boot_wait=1s + <wait3> = ~4s before 'c'.
  # 'c' enters GRUB command line (stops the 30s countdown), then we type
  # kernel + initrd + boot manually with autoinstall + nocloud-net params.
  # ip=dhcp is critical — ensures networking is up to fetch user-data from HTTP.
  boot_wait = "1s"
  boot_command = [
    "<wait3>c<wait3>",
    "linux /casper/vmlinuz quiet autoinstall ip=dhcp ipv6.disable=1 ds=nocloud-net\\;s=http://{{ .HTTPIP }}:{{ .HTTPPort }}/ ---<enter><wait3>",
    "initrd /casper/initrd<enter><wait3>",
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

  provisioner "shell" {
    script = "scripts/install-docker.sh"
  }

  # Snapd removal + aggressive cleanup to minimize image size.
  # live-server installs ~2.5 GB of base packages vs ~500 MB for cloud image.
  provisioner "shell" {
    inline = [
      "sudo DEBIAN_FRONTEND=noninteractive apt-get purge -y snapd ubuntu-server ubuntu-standard || true",
      "sudo DEBIAN_FRONTEND=noninteractive apt-get purge -y linux-firmware wireless-regdb sound-theme-freedesktop || true",
      "sudo DEBIAN_FRONTEND=noninteractive apt-get purge -y fonts-* plymouth* friendly-recovery popularity-contest || true",
      "sudo DEBIAN_FRONTEND=noninteractive apt-get purge -y command-not-found motd-news-config || true",
      "sudo apt-get autoremove -y --purge",
      "sudo rm -rf /var/cache/snapd /snap /usr/share/doc /usr/share/man",
      "sudo rm -rf /usr/share/locale/!(en|en_US|locale.alias)",
      "sudo rm -rf /usr/lib/firmware",
      "sudo apt-get clean"
    ]
  }

  provisioner "shell" {
    script = "scripts/install-sysbox.sh"
    environment_vars = [
      "SYSBOX_VERSION=${var.sysbox_version}",
      "SYSBOX_SHA256=${local.sysbox_sha256}",
      "ARCH=${var.arch}"
    ]
  }

  provisioner "file" {
    source      = var.images_tar
    destination = "/tmp/polis-images.tar"
  }

  provisioner "shell" {
    script = "scripts/load-images.sh"
  }

  provisioner "shell" {
    script = "scripts/harden-vm.sh"
  }

  provisioner "file" {
    source      = var.config_tar
    destination = "/tmp/polis-config.tar.gz"
  }

  provisioner "shell" {
    script = "scripts/install-polis.sh"
  }

  provisioner "file" {
    source      = var.agents_tar
    destination = "/tmp/polis-agents.tar.gz"
  }

  provisioner "shell" {
    script = "scripts/install-agents.sh"
  }

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

  provisioner "shell" {
    inline = [
      # Remove unnecessary packages and caches
      "sudo apt-get purge -y linux-firmware wireless-regdb || true",
      "sudo apt-get autoremove -y --purge",
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*",
      "sudo rm -rf /usr/share/doc /usr/share/man /usr/share/locale/!(en|en_US)",
      "sudo rm -rf /var/cache/* /var/log/*.gz /var/log/*.1 /var/log/journal/*",
      "sudo rm -rf /tmp/* /var/tmp/*",
      # Clean cloud-init
      "sudo cloud-init clean --logs --machine-id --configs network",
      # Lock accounts
      "sudo passwd -l ubuntu",
      "sudo passwd -l root",
      # Zero free space for maximum compaction
      "sudo fstrim -av || true",
      "sudo dd if=/dev/zero of=/EMPTY bs=1M 2>/dev/null || true",
      "sudo rm -f /EMPTY",
      "sudo sync"
    ]
  }
}
