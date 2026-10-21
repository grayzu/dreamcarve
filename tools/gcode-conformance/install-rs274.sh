#!/usr/bin/env bash
#
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Installs LinuxCNC's standalone interpreter, rs274, on Debian 12
# (bookworm), as root, for `cargo run -p gcode-conformance` (issue #173).
#
# rs274 ships in the linuxcnc-uspace package. Ubuntu 22.04 and 24.04 (the
# GitHub runners) have no linuxcnc-uspace, and Debian's own is a 2023
# pre-release (2.9.0~pre1), so this adds LinuxCNC's official apt
# repository, as LinuxCNC's install guide does:
#   https://linuxcnc.org/docs/stable/html/getting-started/getting-linuxcnc.html
#   https://www.linuxcnc.org/linuxcnc-install.sh
# For bookworm that script trusts the "EMC Archive Signing Key"
# (3cb9fd148f374fef) for `https://www.linuxcnc.org/ bookworm base 2.9-uspace`.
# This script does the same with the key's full fingerprint, and pins the
# release the LinuxCNC definition was checked against
# (crates/dc-project/src/definitions/README.md, "LinuxCNC"). The repository
# keeps every 2.9 release, so the pin does not break when a new one lands;
# bump it in its own pull request.

set -euo pipefail

LINUXCNC_VERSION="1:2.9.10"
KEY_FINGERPRINT="EEDD0D29F81DCAA0D258661F3CB9FD148F374FEF"
KEYRING="/usr/share/keyrings/linuxcnc-archive.gpg"
KEYSERVER="hkp://keyserver.ubuntu.com"

. /etc/os-release
if [ "${VERSION_CODENAME:-}" != "bookworm" ]; then
  echo "install-rs274.sh: needs Debian 12 (bookworm), found ${PRETTY_NAME:-unknown}" >&2
  echo "Run it in a debian:bookworm container (see the post-processor checklist)." >&2
  exit 1
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends ca-certificates gnupg >/dev/null

GNUPGHOME="$(mktemp -d)"
export GNUPGHOME
trap 'rm -r -f -- "$GNUPGHOME"' EXIT
for attempt in 1 2 3; do
  if gpg --batch --keyserver "$KEYSERVER" --recv-keys "$KEY_FINGERPRINT"; then
    break
  fi
  if [ "$attempt" = 3 ]; then
    echo "install-rs274.sh: cannot fetch the LinuxCNC archive key" >&2
    exit 1
  fi
  sleep 5
done
# Trust only the key whose full fingerprint is pinned above.
if ! gpg --batch --with-colons --fingerprint "$KEY_FINGERPRINT" | grep -q "^fpr:*${KEY_FINGERPRINT}:"; then
  echo "install-rs274.sh: fetched key does not match $KEY_FINGERPRINT" >&2
  exit 1
fi
gpg --batch --export "$KEY_FINGERPRINT" > "$KEYRING"

echo "deb [signed-by=$KEYRING] https://www.linuxcnc.org/ bookworm base 2.9-uspace" \
  > /etc/apt/sources.list.d/linuxcnc.list
apt-get update -qq
apt-get install -y -qq --no-install-recommends "linuxcnc-uspace=$LINUXCNC_VERSION" >/dev/null

dpkg-query -W linuxcnc-uspace
command -v rs274
