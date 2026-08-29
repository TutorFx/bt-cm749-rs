#!/bin/bash
# Real DKMS install -> verify -> reinstall -> uninstall cycle, run as root inside a
# disposable distro container:  ci/dkms-e2e.sh <path to bt-cm749 binary>
set -euo pipefail

BIN=${1:?usage: $0 <bt-cm749 binary>}
. /etc/os-release
echo "### distro: ${PRETTY_NAME}"

case "${ID}" in
  arch)
    pacman -Syu --noconfirm --needed pahole dkms base-devel linux-headers zstd >/dev/null ;;
  ubuntu|debian)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq && apt-get install -y -qq linux-headers-generic build-essential dkms dwarves kmod zstd >/dev/null ;;
  fedora)
    dnf install -y -q kernel-devel dkms dwarves gcc make elfutils-libelf-devel kmod zstd >/dev/null
    # Without kernel-core in the container, nothing links the headers into /lib/modules.
    for dir in /usr/src/kernels/*; do
      mkdir -p "/usr/lib/modules/${dir##*/}" && ln -sfn "${dir}" "/usr/lib/modules/${dir##*/}/build"
    done ;;
  *) echo "unsupported distro ${ID}"; exit 2 ;;
esac

KV=$(ls /usr/lib/modules | sort -V | tail -1)
export KERNEL_VERSION="${KV}"
echo "### kernel headers: ${KV}"

if ! "${BIN}" install > /tmp/install.log 2>&1; then
  echo "### install FAILED"; tail -40 /tmp/install.log; exit 1
fi
grep -E "Detected kernel|pre_build|Building module|Installing|Installation complete" /tmp/install.log

dkms status | tee /tmp/status
grep -q "bt-cm749/0.3, ${KV}.*installed" /tmp/status

MODULE=$(find "/usr/lib/modules/${KV}/updates" -name 'btusb.ko*' | head -1)
echo "### module: ${MODULE}"
case "${MODULE}" in
  *.zst) zstd -qdc "${MODULE}" ;; *.xz) xz -dc "${MODULE}" ;; *) cat "${MODULE}" ;;
esac | grep -aq "Unexpected continuation"
echo "### module contains the Barrot continuation handler"

"${BIN}" install > /tmp/install2.log 2>&1 || { echo "### reinstall FAILED"; tail -20 /tmp/install2.log; exit 1; }
echo "### reinstall OK"

"${BIN}" uninstall > /tmp/uninstall.log 2>&1
test -z "$(dkms status)"
test ! -e /usr/src/bt-cm749-0.3
test -z "$(find "/usr/lib/modules/${KV}" -path '*updates*' -name 'btusb*' 2>/dev/null)"
echo "### uninstall left no traces"
echo "### E2E PASSED"
