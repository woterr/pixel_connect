#!/usr/bin/env bash
# Sign a built RPM with Pixel Connect's own key.
#
# Usage: packaging/signing/sign-rpm.sh <file.rpm> [...]
set -euo pipefail

if [ $# -eq 0 ]; then
    echo "usage: $0 <file.rpm> [...]" >&2
    exit 2
fi

if ! command -v rpmsign >/dev/null; then
    echo "rpmsign is not installed (Fedora: sudo dnf install rpm-sign)" >&2
    exit 1
fi

for rpm_file in "$@"; do
    [ -f "$rpm_file" ] || { echo "no such file: $rpm_file" >&2; exit 1; }
    rpmsign --addsign "$rpm_file"
    echo "signed $rpm_file"
    rpmsign --verify "$rpm_file"
done
