# Sentinel

[![CI](https://github.com/atayozcan/sentinel/actions/workflows/ci.yml/badge.svg)](https://github.com/atayozcan/sentinel/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/atayozcan/sentinel/badge)](https://scorecard.dev/viewer/?uri=github.com/atayozcan/sentinel)
[![REUSE compliant](https://api.reuse.software/badge/github.com/atayozcan/sentinel)](https://api.reuse.software/info/github.com/atayozcan/sentinel)
[![Latest release](https://img.shields.io/github/v/release/atayozcan/sentinel?include_prereleases&sort=semver)](https://github.com/atayozcan/sentinel/releases/latest)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![MSRV: 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](rust-toolchain.toml)

A Windows UAC-style confirmation dialog for Linux privilege escalation.
A shared PAM + polkit-agent backend with **two desktop frontends** — KDE
Plasma (Kirigami) and COSMIC (libcosmic). Wayland-only, `sudo-rs` friendly.

> **Monorepo note.** This repository unifies the former `sentinel-kde`
> and `sentinel-cosmic` projects: one backend, two frontends, released
> in lockstep. The old repositories are archived and redirect here.

> [!CAUTION]
> Sentinel sits in the **PAM authentication path**. A misconfiguration
> can lock you out of `sudo`, polkit, or login. Read the
> [Troubleshooting](https://atayozcan.github.io/sentinel/troubleshooting.html)
> page **before** you install. Open a second root shell during the
> first install (`pkexec bash`) and keep it open until you've verified
> `sudo` still works.
>
> **Provided as-is, without warranty of any kind.** The author takes
> no responsibility for damaged systems, lost work, or any other
> consequence of running this software. See [LICENSE](LICENSE) (GPL-3.0
> sections 15 and 16). Use on production systems at your own risk.

## Documentation

Full docs live at **<https://atayozcan.github.io/sentinel/>** (built
from `docs/` via mdBook, deployed by `.github/workflows/docs.yml`):

- [Installation](https://atayozcan.github.io/sentinel/installation.html) — AUR, Debian, Fedora, NixOS, generic tarball, source
- [Configuration](https://atayozcan.github.io/sentinel/configuration.html) — `/etc/security/sentinel.conf` reference
- [PAM wiring](https://atayozcan.github.io/sentinel/pam-wiring.html) — `sudo`, `polkit`, `su`
- [Building from source](https://atayozcan.github.io/sentinel/building-from-source.html)
- [Architecture](https://atayozcan.github.io/sentinel/architecture.html) — design and security model
- [Troubleshooting](https://atayozcan.github.io/sentinel/troubleshooting.html) — recovery, common failures, debug logging
- [Contributing](https://atayozcan.github.io/sentinel/contributing.html)
- [Security policy](https://atayozcan.github.io/sentinel/security.html)

## Quick install

Pick the frontend that matches your desktop. Both install the same
backend (PAM module + polkit agent); only the dialog differs.

```bash
# Arch Linux (AUR)
yay -S sentinel-kde        # KDE Plasma (Kirigami dialog)
yay -S sentinel-cosmic     # COSMIC (libcosmic dialog)

# Debian / Ubuntu — COSMIC frontend
curl -LO https://github.com/atayozcan/sentinel/releases/latest/download/sentinel_0.9.0-1_amd64.deb
sudo apt install ./sentinel_0.9.0-1_amd64.deb

# Fedora / openSUSE — COSMIC frontend
curl -LO https://github.com/atayozcan/sentinel/releases/latest/download/sentinel-0.9.0-1.x86_64.rpm
sudo dnf install ./sentinel-0.9.0-1.x86_64.rpm

# NixOS — flake at the repo root
nix run github:atayozcan/sentinel -- --timeout 10 --randomize

# From source
git clone https://github.com/atayozcan/sentinel
cd sentinel
pkexec ./install.sh                 # COSMIC frontend
pkexec ./packaging-kde/install.sh   # KDE Plasma frontend
```

See [Installation](https://atayozcan.github.io/sentinel/installation.html)
for full instructions, including the prebuilt binary tarballs and
per-distro details. Prebuilt bundles are published per release as
`sentinel-<ver>-<arch>-linux.tar.gz` (COSMIC) and
`sentinel-kde-<ver>-<arch>-linux.tar.gz` (KDE).

> **Why `pkexec` for the source install?** The installer needs root
> to write to `/etc/pam.d/`, `/etc/security/`, `/usr/lib/security/`,
> and `/etc/systemd/system/`. `pkexec` routes that elevation through
> polkit (which Sentinel itself can be wired into post-install),
> matches the security model of distros that have phased out `sudo`
> in favour of polkit-mediated elevation, and keeps a clean audit
> trail. `sudo` works too if you prefer.

## What it does

When something requests privilege escalation (`sudo`, `pkexec`, …) and
the PAM stack hits `pam_sentinel.so`, the polkit agent spawns the
frontend helper — `sentinel-helper-kde` on Plasma, `sentinel-helper` on
COSMIC. The helper paints a `zwlr-layer-shell-v1` overlay surface —
full-screen translucent backdrop, exclusive keyboard focus, dialog card
centered — and waits for **Allow**, **Deny**, or a configurable timeout
(auto-deny). Allow → PAM passes auth without a password. Deny / timeout
/ no Wayland display → PAM continues to the next module (typically
`pam_unix`, the password prompt).

The approval is conveyed from the user's agent to `pam_sentinel.so`
(running as root inside `polkit-agent-helper-1`) over the **system
D-Bus** (`org.sentinel.Agent` → `TakeApproval`). D-Bus is used rather
than a unix socket because it rides existing SELinux/AppArmor
permissions (`policykit_t` may `dbus send_msg` but not write an
arbitrary socket), so the bypass works under SELinux out of the box.

## Compatibility

The dialog renders as a `zwlr-layer-shell-v1` overlay on wlroots-style
compositors, falling back to a normal `xdg-toplevel` window on Mutter.

| Compositor    | Status        | Notes |
| ------------- | ------------- | ----- |
| KWin/Wayland  | tested        | Plasma 6.x; the KDE frontend (`sentinel-helper-kde`) registers ahead of polkit-kde |
| cosmic-comp   | tested        | the COSMIC frontend (`sentinel-helper`) |
| Hyprland      | expected to work | sample animation/blur rules at `packaging/hyprland/sentinel.conf` |
| Sway          | expected to work | reference wlroots compositor |
| Niri          | expected to work | layer-shell overlay anchors fullscreen as on other wlroots-style compositors |
| Wayfire       | expected to work | wlroots-based |
| River         | expected to work | wlroots-based |
| GNOME/Mutter  | auto-fallback | Mutter has no `zwlr-layer-shell-v1`. Helper detects via `XDG_CURRENT_DESKTOP` and falls back to `xdg-toplevel` (regular window) automatically; force with `--windowed`. |
| Pantheon / Budgie / Unity | auto-fallback | Same as GNOME — Mutter-based. |
| X11 only      | not supported | Wayland-only |

If you've used Sentinel on a compositor in the "expected to work"
list and want it promoted to "tested", open a PR updating this
table — bonus points for a screenshot.

## Project layout

```
.
├── Cargo.toml                  # workspace root (backend + both frontends)
├── crates/
│   ├── sentinel-shared/        # shared schema, parser, /proc + logind readers, log_kv
│   ├── pam-sentinel/           # cdylib → /usr/lib/security/pam_sentinel.so
│   ├── sentinel-polkit-agent/  # bin    → /usr/lib/sentinel-polkit-agent (D-Bus bypass)
│   ├── sentinel-helper/        # COSMIC / libcosmic frontend → /usr/lib/sentinel-helper
│   │   └── locales/            # 12 embedded fluent bundles (en-US, de-DE, …)
│   └── sentinel-helper-kde/    # KDE Plasma / Kirigami (cxx-qt) frontend → /usr/lib/sentinel-helper-kde
├── config/                     # /etc/security/sentinel.conf, /etc/pam.d/{polkit-1,sudo}
├── packaging/                  # COSMIC: Arch PKGBUILD, debian, systemd, xdg, dbus, FLATPAK rationale
├── packaging-kde/              # KDE frontend: install.sh, PKGBUILD, packaging, build-release.sh
├── nix/module.nix              # NixOS module
├── flake.nix
├── scripts/build-release.sh    # COSMIC source + binary tarballs
├── install.sh / uninstall.sh   # transactional installer (auto-rollback, in-place agent restart)
└── .github/workflows/
    ├── ci.yml                  # fmt + clippy + test + build (both frontends) on PRs
    └── release.yml             # tag v* → builds both bundles + one GH release + both AUR
```

The backend (`pam-sentinel`, `sentinel-shared`, `sentinel-polkit-agent`)
is kept out of `default-members`'s GUI deps, so a bare `cargo build`
compiles the pure-Rust auth path without pulling Qt or libcosmic. Build
a frontend explicitly with `cargo build -p sentinel-helper[-kde]`.

## License

**GPL-3.0-or-later.** See [LICENSE](LICENSE). GPL-3.0 sections 15 and
16 disclaim all warranty and limit liability.
