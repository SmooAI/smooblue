#!/usr/bin/env bash
# One-time (and key-rotation) setup of the macOS signing + Sparkle
# secrets on SmooAI/smooblue. Run on the release manager's Mac — the one
# holding the "Developer ID Application: Smoo LLC" identity, the App
# Store Connect API key, and the "Smooblue" Sparkle key (generate_keys
# --account Smooblue). See docs/Operations/Sparkle-Updates.md.
#
#   scripts/set-release-secrets.sh
#
# macOS will ask (GUI) to allow exporting the Developer ID private key
# from the login keychain — that's expected. Only the Developer ID
# identity goes into MACOS_CERT_P12: `security export` dumps every
# identity in the keychain, so the matching cert + key are picked out
# and re-packed on their own (the Apple Distribution identity, if
# present, is dropped). Nothing secret is printed; every temp file is
# removed on exit.

set -euo pipefail

REPO="SmooAI/smooblue"
IDENTITY="Developer ID Application: Smoo LLC (DTX9733844)"
NOTARY_ISSUER="334efd73-f035-487f-bbe2-9a29cf4f4d97"
SPARKLE_BIN="${SMOOBLUE_SPARKLE_CACHE:-$HOME/.cache/smooblue}/sparkle-2.9.6/bin"
OPENSSL=/usr/bin/openssl # LibreSSL: reads the keychain's legacy PKCS#12 without -legacy

WORK="$(mktemp -d)"
chmod 700 "$WORK"
trap 'rm -rf "$WORK"' EXIT

set_secret() { # name value
    gh secret set "$1" -R "$REPO" --body "$2" >/dev/null
    echo "  ✓ $1"
}

echo "▸ exporting identities from the login keychain (approve the macOS prompt)"
EXPORT_PASS="$($OPENSSL rand -hex 16)"
security export -k "$HOME/Library/Keychains/login.keychain-db" -t identities -f pkcs12 \
    -P "$EXPORT_PASS" -o "$WORK/all.p12"
$OPENSSL pkcs12 -in "$WORK/all.p12" -passin "pass:$EXPORT_PASS" -nodes -out "$WORK/all.pem" 2>/dev/null

# Split the PEM bundle into one file per cert / key.
awk -v dir="$WORK" '
    /-----BEGIN CERTIFICATE-----/ { n++; f = dir "/cert" n ".pem" }
    /-----BEGIN .*PRIVATE KEY-----/ { k++; f = dir "/key" k ".pem" }
    f { print > f }
    /-----END/ { close(f); f = "" }
' "$WORK/all.pem"

CERT=""
for c in "$WORK"/cert*.pem; do
    if $OPENSSL x509 -in "$c" -noout -subject | grep -q "Developer ID Application: Smoo LLC"; then
        CERT="$c"
    fi
done
[ -n "$CERT" ] || { echo "error: no '$IDENTITY' certificate in the export" >&2; exit 1; }
WANT="$($OPENSSL x509 -in "$CERT" -noout -pubkey)"
KEY=""
for k in "$WORK"/key*.pem; do
    if [ "$($OPENSSL pkey -in "$k" -pubout 2>/dev/null)" = "$WANT" ]; then
        KEY="$k"
    fi
done
[ -n "$KEY" ] || { echo "error: private key for '$IDENTITY' not found in the export" >&2; exit 1; }

P12_PASS="$($OPENSSL rand -hex 24)"
$OPENSSL pkcs12 -export -in "$CERT" -inkey "$KEY" -name "$IDENTITY" \
    -passout "pass:$P12_PASS" -out "$WORK/devid.p12"
# Round-trip check: exactly one cert, and it's the Developer ID one.
$OPENSSL pkcs12 -in "$WORK/devid.p12" -passin "pass:$P12_PASS" -nokeys 2>/dev/null \
    | grep -q "Developer ID Application: Smoo LLC" || { echo "error: re-packed .p12 failed verification" >&2; exit 1; }

echo "▸ exporting the Smooblue Sparkle key"
"$SPARKLE_BIN/generate_keys" --account Smooblue -x "$WORK/sparkle.key" >/dev/null

NOTARY_KEY="$(ls "$HOME"/.appstoreconnect/private_keys/AuthKey_*.p8 | head -n1)"
[ -f "$NOTARY_KEY" ] || { echo "error: no AuthKey_*.p8 in ~/.appstoreconnect/private_keys" >&2; exit 1; }
NOTARY_KEY_ID="$(basename "$NOTARY_KEY" .p8)"
NOTARY_KEY_ID="${NOTARY_KEY_ID#AuthKey_}"

echo "▸ setting secrets on $REPO"
set_secret MACOS_CERT_P12 "$(base64 -i "$WORK/devid.p12" | tr -d '\n')"
set_secret MACOS_CERT_PASSWORD "$P12_PASS"
set_secret MACOS_SIGN_IDENTITY "$IDENTITY"
set_secret NOTARY_KEY_P8 "$(base64 -i "$NOTARY_KEY" | tr -d '\n')"
set_secret NOTARY_KEY_ID "$NOTARY_KEY_ID"
set_secret NOTARY_ISSUER "$NOTARY_ISSUER"
set_secret SMOOBLUE_SPARKLE_PRIVATE_KEY "$(cat "$WORK/sparkle.key")"

echo ""
echo "✓ Done. Temp files removed. Current secrets on $REPO:"
gh secret list -R "$REPO" | cut -f1
