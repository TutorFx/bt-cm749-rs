#!/bin/bash
# Exercises install.sh the way a user runs it, inside a disposable Ubuntu 24.04
# container, against locally served release files:
#   ci/installer-test.sh <path to bt-cm749-x86_64-linux-musl>
# Covers a corrupted download, the "kernel already has the fix" path (Ubuntu's
# own btusb ships it) and uninstallation.
set -euo pipefail

BIN=${1:?usage: $0 <bt-cm749 binary>}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ASSET=bt-cm749-x86_64-linux-musl

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq curl ca-certificates dkms linux-headers-generic >/dev/null
KV=$(find /usr/lib/modules -mindepth 1 -maxdepth 1 -printf '%f\n' | sort -V | tail -1)
apt-get install -y -qq "linux-modules-${KV}" >/dev/null
apt-get install -y -qq "linux-modules-extra-${KV}" >/dev/null 2>&1 || true
export KERNEL_VERSION="${KV}"

REL=$(mktemp -d)
mkdir "${REL}/good" "${REL}/bad"
cp "${BIN}" "${REL}/good/${ASSET}"
(cd "${REL}/good" && sha256sum "${ASSET}" > "${ASSET}.sha256")
cp "${REL}/good/"* "${REL}/bad/"
printf 'corrupted' >> "${REL}/bad/${ASSET}"

echo "### corrupted download is rejected"
if BT_CM749_BASE_URL="file://${REL}/bad" sh "${ROOT}/install.sh"; then
  echo "### FAILED: a corrupted download was accepted"; exit 1
fi
test ! -e /usr/local/bin/bt-cm749

echo "### install on a kernel that already has the fix"
BT_CM749_BASE_URL="file://${REL}/good" sh "${ROOT}/install.sh" | tee /tmp/install.log
grep -q "nothing had to be installed" /tmp/install.log
test -x /usr/local/bin/bt-cm749
test -z "$(dkms status)"

echo "### uninstall"
sh "${ROOT}/install.sh" --uninstall
test ! -e /usr/local/bin/bt-cm749
echo "### INSTALLER TEST PASSED"
