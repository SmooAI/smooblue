#!/usr/bin/env python3
"""Write the Sparkle appcast for one Smooblue release.

Sparkle only needs the newest item, so the feed is rebuilt from scratch
each release (same as SmoothFlow's) and uploaded as the `appcast.xml`
asset of the GitHub release. The app's SUFeedURL is
`releases/latest/download/appcast.xml`, which GitHub redirects to the
newest non-prerelease release — i.e. this file.

    scripts/make-appcast.py --version 1.30.0 \\
        --zip dist/Smooblue-macos-arm64.zip \\
        --url https://github.com/SmooAI/smooblue/releases/download/v1.30.0/Smooblue-macos-arm64.zip \\
        [--key-file sparkle.key] [--out dist/appcast.xml]

The enclosure is EdDSA-signed with Sparkle's own `sign_update` (from the
same pinned Sparkle build bundle-macos.sh embeds). Without --key-file it
signs with the "Smooblue" account in the login keychain
(`generate_keys --account Smooblue`). Release notes are that version's
CHANGELOG.md section as Markdown (rendered by Sparkle on macOS 12+).
"""

from __future__ import annotations

import argparse
import base64
import email.utils
import hashlib
import os
import plistlib
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SPARKLE_VERSION = "2.9.6"
MIN_MACOS = "11.0"
RELEASES = "https://github.com/SmooAI/smooblue/releases"

# "- [#93](…) [`7454556`](…) Thanks [@brentrager](…)! - Text" → "- Text"
CHANGESET_PREFIX = re.compile(
    r"^- (?:\[#\d+\]\([^)]*\) )?(?:\[`[0-9a-f]+`\]\([^)]*\) )?(?:Thanks \[@[^\]]+\]\([^)]*\)! )?- "
)


def changelog_notes(changelog: str, version: str) -> str:
    """The `## <version>` section, tidied for end users: changeset
    attribution stripped, the "### Patch Changes"-style headings dropped
    (Sparkle's dialog already says what version this is)."""
    lines = changelog.splitlines()
    out: list[str] = []
    found = False
    for line in lines:
        if re.fullmatch(rf"## {re.escape(version)}\s*", line):
            found = True
            continue
        if found and re.match(r"## \d+\.", line):
            break
        if not found:
            continue
        if re.fullmatch(r"### (Major|Minor|Patch) Changes\s*", line):
            continue
        out.append(CHANGESET_PREFIX.sub("- ", line))
    text = "\n".join(out).strip()
    return text or f"Smooblue {version}. Full notes: {RELEASES}/tag/v{version}"


def sign(zip_path: Path, key_file: str | None, sign_update: Path) -> tuple[str, str]:
    """(edSignature, length) for the archive, via Sparkle's sign_update."""
    cmd = [str(sign_update)]
    if key_file:
        cmd += ["--ed-key-file", key_file]
    else:
        cmd += ["--account", "Smooblue"]
    cmd.append(str(zip_path))
    out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    sig = re.search(r'sparkle:edSignature="([^"]+)"', out)
    length = re.search(r'length="(\d+)"', out)
    if not sig or not length:
        sys.exit(f"sign_update produced no signature: {out!r}")
    return sig.group(1), length.group(1)


# ── Ed25519 verification (RFC 8032 §6 reference) ─────────────────────
# Dependency-free so CI can run it on a bare runner Python. It checks
# the signature against the SUPublicEDKey baked into the app — the
# check that matters: a CI key that doesn't match the shipped public
# key produces updates no installed app will accept, and sign_update
# --verify (which derives the public half from the same private key)
# can't catch that.
_P = 2**255 - 19
_Q = 2**252 + 27742317777372353535851937790883648493
_D = -121665 * pow(121666, _P - 2, _P) % _P
_SQRT_M1 = pow(2, (_P - 1) // 4, _P)


def _recover_x(y: int, sign: int) -> int | None:
    if y >= _P:
        return None
    x2 = (y * y - 1) * pow(_D * y * y + 1, _P - 2, _P)
    if x2 % _P == 0:
        return None if sign else 0
    x = pow(x2, (_P + 3) // 8, _P)
    if (x * x - x2) % _P != 0:
        x = x * _SQRT_M1 % _P
    if (x * x - x2) % _P != 0:
        return None
    if (x & 1) != sign:
        x = _P - x
    return x


def _add(a, b):
    A = (a[1] - a[0]) * (b[1] - b[0]) % _P
    B = (a[1] + a[0]) * (b[1] + b[0]) % _P
    C = 2 * a[3] * b[3] * _D % _P
    D = 2 * a[2] * b[2] % _P
    E, F, G, H = B - A, D - C, D + C, B + A
    return (E * F % _P, G * H % _P, F * G % _P, E * H % _P)


def _mul(s: int, pt):
    acc = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            acc = _add(acc, pt)
        pt = _add(pt, pt)
        s >>= 1
    return acc


def _decompress(raw: bytes):
    y = int.from_bytes(raw, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    return None if x is None else (x, y, 1, x * y % _P)


_GY = 4 * pow(5, _P - 2, _P) % _P
_G = (_recover_x(_GY, 0), _GY, 1, _recover_x(_GY, 0) * _GY % _P)


def ed25519_verify(public: bytes, message: bytes, signature: bytes) -> bool:
    if len(public) != 32 or len(signature) != 64:
        return False
    a = _decompress(public)
    r = _decompress(signature[:32])
    s = int.from_bytes(signature[32:], "little")
    if a is None or r is None or s >= _Q:
        return False
    h = int.from_bytes(hashlib.sha512(signature[:32] + public + message).digest(), "little") % _Q
    left, right = _mul(s, _G), _add(r, _mul(h, a))
    return (left[0] * right[2] - right[0] * left[2]) % _P == 0 and (
        left[1] * right[2] - right[1] * left[2]
    ) % _P == 0


def check_signature(archive: Path, signature_b64: str, public_key_b64: str) -> None:
    ok = ed25519_verify(
        base64.b64decode(public_key_b64),
        archive.read_bytes(),
        base64.b64decode(signature_b64),
    )
    if not ok:
        sys.exit(
            f"EdDSA signature of {archive.name} does NOT verify against SUPublicEDKey "
            f"{public_key_b64} — installed apps would reject this update. Is the signing "
            "key the Smooblue key?"
        )


def app_public_key(app: Path) -> str:
    with open(app / "Contents" / "Info.plist", "rb") as f:
        return plistlib.load(f)["SUPublicEDKey"]


def render(version: str, url: str, signature: str, length: str, notes: str, pub_date: str) -> str:
    notes = notes.replace("]]>", "]]&gt;")
    return f"""<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
    <channel>
        <title>Smooblue</title>
        <link>{RELEASES}</link>
        <description>Smooblue releases</description>
        <language>en</language>
        <item>
            <title>Smooblue {version}</title>
            <pubDate>{pub_date}</pubDate>
            <sparkle:version>{version}</sparkle:version>
            <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
            <sparkle:minimumSystemVersion>{MIN_MACOS}</sparkle:minimumSystemVersion>
            <sparkle:fullReleaseNotesLink>{RELEASES}/tag/v{version}</sparkle:fullReleaseNotesLink>
            <description sparkle:format="markdown"><![CDATA[
{notes}
]]></description>
            <enclosure url="{url}" sparkle:edSignature="{signature}" length="{length}" type="application/octet-stream"/>
        </item>
    </channel>
</rss>
"""


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--version")
    p.add_argument("--zip", required=True, type=Path)
    p.add_argument("--url")
    p.add_argument(
        "--app",
        type=Path,
        default=REPO / "dist" / "Smooblue.app",
        help="app whose SUPublicEDKey the signature must verify against",
    )
    p.add_argument(
        "--check",
        type=Path,
        metavar="APPCAST",
        help="only verify APPCAST's enclosure signature over --zip (post-publish check)",
    )
    p.add_argument("--key-file")
    p.add_argument("--changelog", type=Path, default=REPO / "CHANGELOG.md")
    p.add_argument("--out", type=Path, default=REPO / "dist" / "appcast.xml")
    default_cache = Path(os.environ.get("SMOOBLUE_SPARKLE_CACHE", Path.home() / ".cache" / "smooblue"))
    p.add_argument("--sign-update", type=Path, default=default_cache / f"sparkle-{SPARKLE_VERSION}" / "bin" / "sign_update")
    a = p.parse_args()

    if not a.zip.is_file():
        sys.exit(f"archive not found: {a.zip}")
    public_key = app_public_key(a.app)
    if a.check:
        found = re.search(r'sparkle:edSignature="([^"]+)"', a.check.read_text())
        if not found:
            sys.exit(f"no sparkle:edSignature in {a.check}")
        check_signature(a.zip, found.group(1), public_key)
        print(f"✓ {a.zip.name} verifies against SUPublicEDKey {public_key}")
        return
    if not a.version or not a.url:
        p.error("--version and --url are required unless --check is given")
    if not a.sign_update.is_file():
        sys.exit(f"sign_update not found at {a.sign_update} — run scripts/bundle-macos.sh first")
    signature, length = sign(a.zip, a.key_file, a.sign_update)
    check_signature(a.zip, signature, public_key)
    notes = changelog_notes(a.changelog.read_text(), a.version)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(render(a.version, a.url, signature, length, notes, email.utils.formatdate(usegmt=True)))
    print(f"✓ {a.out} — v{a.version}, {length} bytes, signed")


if __name__ == "__main__":
    main()
