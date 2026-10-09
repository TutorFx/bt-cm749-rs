#!/bin/bash
# Real DKMS install -> verify -> reinstall -> uninstall cycle, run as root inside a
# disposable distro container:  ci/dkms-e2e.sh <path to bt-cm749 binary>
#
# On Debian/Ubuntu the kernel's own modules are installed too; their btusb already
# carries the Barrot fix, so the expected outcome there is a verified skip.
set -euo pipefail

BIN=${1:?usage: $0 <bt-cm749 binary>}
. /etc/os-release
echo "### distro: ${PRETTY_NAME}"

case "${ID}" in
  arch)
    pacman -Syu --noconfirm --needed pahole dkms base-devel linux-headers zstd >/dev/null ;;
  ubuntu|debian)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq && apt-get install -y -qq linux-headers-generic build-essential dkms dwarves kmod zstd >/dev/null
    kv=$(ls /usr/lib/modules | sort -V | tail -1)
    apt-get install -y -qq "linux-modules-${kv}" >/dev/null
    apt-get install -y -qq "linux-modules-extra-${kv}" >/dev/null 2>&1 || true ;;
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

"${BIN}" detect | sed 's/^/    /'

if [ "${ID}" = ubuntu ] || [ "${ID}" = debian ]; then
  "${BIN}" install > /tmp/install.log 2>&1 || { echo "### install FAILED"; tail -20 /tmp/install.log; exit 1; }
  grep -q "Nothing to install" /tmp/install.log
  test -z "$(dkms status)"
  test ! -e /usr/src/bt-cm749-0.3
  echo "### stock btusb already fixed: install skipped without side effects"
  echo "### E2E PASSED"
  exit 0
fi

# Without the distro's kernel package in the container the stock driver cannot be
# inspected, so this exercises the full DKMS path; --force keeps it that way.
if ! "${BIN}" install --force > /tmp/install.log 2>&1; then
  echo "### install FAILED"; tail -40 /tmp/install.log; exit 1
fi
grep -E "Detected kernel|pre_build|Building module|Installing|Installation complete" /tmp/install.log

dkms status | tee /tmp/status
grep -q "bt-cm749/0.3, ${KV}.*installed" /tmp/status

# DKMS installs to updates/dkms, except on Fedora where its policy forces extra/.
find_dkms_btusb() { find "/usr/lib/modules/${KV}" \( -path '*/updates/*' -o -path '*/extra/*' \) -name 'btusb.ko*'; }
MODULE=$(find_dkms_btusb | head -1)
test -n "${MODULE}"
echo "### module: ${MODULE}"
case "${MODULE}" in
  *.zst) zstd -qdc "${MODULE}" ;; *.xz) xz -dc "${MODULE}" ;; *) cat "${MODULE}" ;;
esac | grep -a "Unexpected continuation" >/dev/null  # no -q: it would SIGPIPE the producer under pipefail
echo "### module contains the Barrot continuation handler"

"${BIN}" install --force > /tmp/install2.log 2>&1 || { echo "### reinstall FAILED"; tail -20 /tmp/install2.log; exit 1; }
echo "### reinstall OK"

"${BIN}" uninstall > /tmp/uninstall.log 2>&1
test -z "$(dkms status)"
test ! -e /usr/src/bt-cm749-0.3
test -z "$(find_dkms_btusb)"
echo "### uninstall left no traces"
echo "### E2E PASSED"
