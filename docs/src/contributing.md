# Contributing

Thanks for thinking about contributing! Sentinel sits in the PAM
authentication path, so reviewers are pickier than average — but the flow
itself is normal GitHub fork-PR-merge.

## Development quickstart

```bash
sudo zypper install rustup pam-devel        # + Qt6/KF6 devel for the helper
rustup default 1.85

git clone https://github.com/atayozcan/sentinel-kde
cd sentinel-kde

cargo build --release --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The helper (`sentinel-helper-kde`) builds against Qt 6 + KDE Frameworks 6 via
cxx-qt; on aarch64 the bundled `.cargo/config.toml` pins `ld.bfd` (lld rejects
Qt's `--fix-cortex-a53-835769`).

## Things reviewers check

- **Sentinel sits in the PAM auth path.** If you touch `pam-sentinel`, the
  agent, or the helper, run `sudo ./install.sh && pkexec true` end-to-end
  before opening the PR — a regression here can lock people out of `sudo`/polkit.
- **Install / uninstall** — run the podman suite, which covers the rollback and
  reinstall paths:
  ```bash
  cargo build --release -p pam-sentinel -p sentinel-polkit-agent -p sentinel-helper-kde
  ./scripts/test-install-container.sh        # 9 scenarios
  ```
- **SELinux.** The bypass is designed to need no custom SELinux policy; if you
  change the agent↔PAM channel, verify it still works under `setenforce 1`.
- **SPDX headers** on new files (`reuse lint` enforces the convention).

## Architecture references

- [Architecture](./architecture.md) — design and trust boundaries.
- [Configuration](./configuration.md) — the on-disk schema.
- [PAM wiring](./pam-wiring.md) — install-time semantics.

## Reporting

General bugs: <https://github.com/atayozcan/sentinel-kde/issues>. Security
issues go through GitHub Private Vulnerability Reporting — see the
[security policy](./security.md).

## License

By contributing you agree your changes ship under **GPL-3.0-or-later**.
