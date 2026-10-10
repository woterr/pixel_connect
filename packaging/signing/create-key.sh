#!/usr/bin/env bash
# Create Pixel Connect's own RPM/release signing key.
#
# Generates a dedicated GPG subkey and prints its fingerprint. Nothing is
# written into this directory: the key lives in the user's GPG keyring, and only
# the fingerprint is meant to be published.
#
# Upstream rquickshare's signing identity is never reused.
set -euo pipefail

NAME="${PIXEL_CONNECT_SIGNING_NAME:-Pixel Connect <dev@pixelconnect.app>}"
TTL="${PIXEL_CONNECT_SIGNING_TTL:-2y}"

if gpg --list-secret-keys --with-colons 2>/dev/null | grep -q '^fpr:.*:.*:.*:.*:.*:$'; then
    echo "A signing key already exists; refusing to create a second one." >&2
    gpg --list-secret-keys --keyid-format long >&2
    exit 1
fi

echo "Creating a signing key for: $NAME"
gpg --quick-generate-key "$NAME" rsa4096 sign "$TTL" never \
    "Pixel Connect RPM and release signing key. Never used for upstream projects."

# Create a dedicated subkey so the primary key never needs a secret on disk.
gpg --quick-add-key "$(gpg --list-secret-keys --with-colons "$NAME" | awk -F: '/^fpr:/ {print $10; exit}')" \
    rsa4096 sign "$TTL" never

echo
echo "Fingerprint (record this in pixel-connect.spec):"
gpg --list-secret-keys --with-colons "$NAME" | awk -F: '/^fpr:/ {print "  " $10}'
echo
echo "Export the public key for publication:"
echo "  gpg --armor --export '$NAME' > pixel-connect-signing-key.asc"
