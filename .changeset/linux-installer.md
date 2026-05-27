---
"smooblue": minor
---

Linux x86_64 release builds + one-line installer.

The release workflow now has a second job that compiles a Linux x86_64 binary on ubuntu-latest and uploads `Smooblue-linux-x86_64.tar.gz` (binary + icon + README) as a release asset alongside the macOS .app.

`install.sh` auto-detects platform and pulls the right asset:

```bash
curl -fsSL https://raw.githubusercontent.com/SmooAI/smooblue/main/install.sh | bash
```

On Linux it installs the binary to `~/.local/bin/smooblue`, drops a `.desktop` entry into `~/.local/share/applications/`, copies the icon into the hicolor theme, refreshes the desktop database, and prints the runtime-deps apt line (webkit2gtk-4.1 / gtk-3 / libayatana-appindicator / librsvg).
