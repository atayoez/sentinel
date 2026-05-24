# Installation

Sentinel-KDE targets **openSUSE Tumbleweed + KDE Plasma 6 (Wayland)**. The
primary install path is the source installer (`install.sh`); a prebuilt binary
bundle and a helper-only RPM are also available.

> **Before you install:** Sentinel sits in the PAM auth path. Open a second
> root shell first (`sudo -i`) and keep it open until you've confirmed `sudo`
> and `pkexec` still work. Sentinel wires in *prepend-in-place* so a broken
> module always falls through to a password, but keep the rescue shell anyway.
> The [Troubleshooting](./troubleshooting.md) page covers recovery.

## From source (recommended)

```bash
sudo zypper install rustup pam-devel
rustup default 1.85

git clone https://github.com/atayozcan/sentinel-kde
cd sentinel-kde
sudo ./install.sh
```

Flags:

| Flag | Effect |
|------|--------|
| *(none)* | Guards polkit **and** `sudo`/`sudo-i`/`su`; reuses prebuilt `target/release` if present, else builds. |
| `--no-sudo` | Guard polkit only; leave `sudo`/`su` as plain password prompts. |
| `--rebuild` | Force a `cargo build` even if `target/release` artifacts exist. |
| `-v` | Verbose (print the installed-file summary). |

Verify:

```bash
pkexec true     # one Sentinel dialog; Allow → no password, exit 0
```

## Prebuilt binary bundle

`scripts/build-release.sh` produces `dist/sentinel-kde-<ver>-<arch>-linux.tar.gz`
— the prebuilt binaries plus `install.sh`. On the target machine:

```bash
tar xzf sentinel-kde-<ver>-<arch>-linux.tar.gz
cd sentinel-kde-<ver>
sudo ./install.sh        # reuses the bundled binaries (no toolchain needed)
```

## RPM (helper only)

```bash
cargo generate-rpm -p crates/sentinel-helper-kde
sudo zypper install ./target/generate-rpm/sentinel-helper-kde-*.rpm
```

The RPM installs **only** `sentinel-helper-kde` (the dialog) plus its Qt/KDE
runtime dependencies. The PAM/polkit/systemd wiring is done by `install.sh`;
the RPM does not wire the auth path.

## What the installer does

1. Reverts any previous Sentinel install first (safe reinstall / repair).
2. Reuses prebuilt `target/release` artifacts if present, otherwise builds the
   workspace **as the invoking user** (the `cargo target/` stays user-owned).
3. Installs `pam_sentinel.so` into the distro PAM module dir
   (`/usr/lib64/security`, auto-detected), the agent, and the helper.
4. Prepends `pam_sentinel.so` into the polkit-1 (and, by default,
   `sudo`/`sudo-i`/`su`) PAM stacks — *in place*, preserving the distro's own
   stack (see [PAM wiring](./pam-wiring.md)).
5. Installs the systemd **user** service + the `org.sentinel.Agent` D-Bus
   policy, masks `plasma-polkit-agent.service`, and adds the polkit admin rule.
6. Records every change in `/var/lib/sentinel/install.state`; verifies
   modes/owners; rolls back automatically on any error.
7. Activates the agent (systemd `--user`) so it takes effect without logout.

## Uninstall

```bash
sudo ./uninstall.sh
```

Replays `/var/lib/sentinel/install.state` in reverse: disables the agent,
unmasks and restarts `plasma-polkit-agent.service`, removes installed files,
restores any backed-up originals, and reloads the bus. Idempotent, with a
best-effort path-based fallback if the state file is missing.
