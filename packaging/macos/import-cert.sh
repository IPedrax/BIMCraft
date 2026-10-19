#!/usr/bin/env bash
# CI only: import the Developer ID certificate into a temporary keychain, for codesign.
#
# Takes (env): APPLE_CERTIFICATE (a base64 .p12), APPLE_CERTIFICATE_PASSWORD, KEYCHAIN_PASSWORD
# (optional; random if unset), RUNNER_TEMP (or TMPDIR).
# Gives: MACOS_SIGN_IDENTITY (the identity's SHA-1) and MACOS_KEYCHAIN, appended to $GITHUB_ENV in
# Actions, printed otherwise.
#
# With no certificate it warns and exits 0, and package.sh signs ad hoc.
set -euo pipefail
umask 077
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

if [ -z "${APPLE_CERTIFICATE:-}" ]; then
  warn "APPLE_CERTIFICATE is not set: the macOS build is signed ad hoc and not notarized"
  exit 0
fi

TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="$TMP/bimcraft-signing.keychain-db"
CERT="$TMP/bimcraft-signing.p12"
KC_PASS="${KEYCHAIN_PASSWORD:-$(openssl rand -hex 24)}"

trap 'rm -f "$CERT"' EXIT
printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$CERT"
security create-keychain -p "$KC_PASS" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KC_PASS" "$KEYCHAIN"
security import "$CERT" -P "${APPLE_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign -T /usr/bin/security -t cert -f pkcs12 -k "$KEYCHAIN"
rm -f "$CERT"
# codesign may use the key without asking.
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PASS" "$KEYCHAIN" >/dev/null
# On the search list, beside the login keychain, so codesign and notarytool find it.
# shellcheck disable=SC2046
security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | awk '/Developer ID Application/ { print $2; exit }')"
if [ -z "$IDENTITY" ]; then
  security find-identity -v -p codesigning "$KEYCHAIN" >&2
  echo "error: APPLE_CERTIFICATE holds no 'Developer ID Application' identity" >&2
  exit 1
fi
echo "imported signing identity $IDENTITY"
if [ -n "${GITHUB_ENV:-}" ]; then
  { echo "MACOS_SIGN_IDENTITY=$IDENTITY"; echo "MACOS_KEYCHAIN=$KEYCHAIN"; } >>"$GITHUB_ENV"
else
  echo "MACOS_SIGN_IDENTITY=$IDENTITY"
  echo "MACOS_KEYCHAIN=$KEYCHAIN"
fi
