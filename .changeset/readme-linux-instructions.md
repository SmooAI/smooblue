---
"smooblue": patch
---

README: split Install into per-platform sections. Adds Linux build instructions (webkit2gtk prerequisites, `cargo run --release` to launch) with honest caveats about macOS-only niceties (Apple Vision OCR, pbcopy-based copy-link, bundle-macos.sh) that degrade gracefully when missing. Notes Windows as theoretically buildable but untested.
