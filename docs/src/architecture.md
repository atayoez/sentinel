# Architecture

Sentinel is a shared backend (a PAM module, a polkit agent, and a
shared crate) plus two interchangeable GUI frontends, in a single Cargo
workspace.

```
crates/
├── sentinel-shared/        # config schema, /proc + logind readers,
│                           # Outcome wire enum, log_kv helpers,
│                           # POLKIT_PAM_SERVICE const, audit::init_syslog
├── pam-sentinel/           # cdylib → /usr/lib/security/pam_sentinel.so
├── sentinel-polkit-agent/  # bin → /usr/lib/sentinel-polkit-agent
├── sentinel-helper/        # COSMIC / libcosmic dialog → /usr/lib/sentinel-helper
└── sentinel-helper-kde/    # KDE Plasma / Kirigami (cxx-qt) dialog → /usr/lib/sentinel-helper-kde
```

The two helpers are drop-in alternatives — same CLI contract, same
`ALLOW`/`DENY`/`TIMEOUT` wire output — so the backend spawns whichever
is installed. The backend stays out of the GUI dependency graph: a bare
`cargo build` compiles the auth path without Qt or libcosmic.

## The PAM module — `pam_sentinel.so`

Loaded by libpam on every authentication attempt for whatever
services have it wired in. For each call it picks one of:

- **Bypass:** the polkit agent has already pre-approved this auth;
  consume it over D-Bus (`org.sentinel.Agent`). Return `PAM_SUCCESS`
  immediately.
- **Dialog:** spawn the frontend helper (`sentinel-helper-kde` or
  `sentinel-helper`), wait for Allow / Deny / timeout. Return
  `PAM_SUCCESS` on Allow, `PAM_AUTH_ERR` otherwise.
- **Headless:** no Wayland display detected. Return whatever
  `headless_action` says (default `PAM_IGNORE` → password prompt).
- **Disabled:** `enabled = false` in config → `PAM_IGNORE`.

Identifying the requesting user uses `/proc/<ppid>/loginuid` (set by
PAM at login, inherited through forks, immune to setuid). Falls back
to `/proc/<ppid>/status` `Uid:` line, then `getuid()`.

The displayed process name uses `/proc/<pid>/cmdline` of the
privileged binary (sudo, pkexec, helper-1) and strips the elevation
wrapper via `sentinel_shared::strip_elevation_prefix`. For wrappers
with no target argv (`sudo -v` for cred-cache), it walks `PPid` to
the calling process so the dialog shows the user-facing originator
(`paru`, `topgrade`) rather than `sudo-rs`.

## The polkit agent — `sentinel-polkit-agent`

A per-user agent that registers with polkitd as the session's
`org.freedesktop.PolicyKit1.AuthenticationAgent`. Forks
`sentinel-helper` for the dialog, then satisfies polkit's cookie
validation via `polkit-agent-helper-1` over its socket.

### Bypass channel (system D-Bus)

The agent claims `org.sentinel.Agent` on the **system** bus and exposes
a `TakeApproval` method. When the agent's own helper-1 invocation runs,
the `pam_sentinel.so` inside it (running as root) calls `TakeApproval`,
gets a one-shot `true` / `false`, and short-circuits to `PAM_SUCCESS`
without spawning a second dialog.

D-Bus — not a unix socket — because `polkit-agent-helper-1` runs as
`policykit_t` under SELinux, which is denied writing an arbitrary
socket but **is** permitted `dbus send_msg` to user domains (the same
path `pam_fprintd` uses). The bypass therefore works under SELinux
(openSUSE Tumbleweed, etc.) with no custom policy. The system-bus
policy in `packaging/dbus/org.sentinel.Agent.conf` lets any user own
the name but only root send to it.

Per-call check:
1. The caller (`pam_sentinel`) verifies the owner uid of
   `org.sentinel.Agent` matches the user being authenticated
   (`GetConnectionUnixUser`), defeating a same-name squatter from
   another uid.
2. The bus policy permits only `root` to call `TakeApproval`.

Approvals are one-shot, expire after 1 second, and `cancel-authentication`
drains the queue so a stale approval can't be picked up by a
racing auth.

### Identity selection

`unix-user` identities are preferred over groups; the matching uid
wins over alternatives; first non-root unix-user is the fallback.
See `crates/sentinel-polkit-agent/src/identity.rs`.

### Why XDG autostart, not systemd-user

The agent must inherit the kernel sessionid of the user's compositor.
A `systemd --user` unit would run under `user@<uid>.service` (a
DIFFERENT sessionid), and polkit's `RegisterAuthenticationAgent`
rejects the mismatch with "Passed session and the session the caller
is in differs". Sentinel's autostart entry sets
`X-systemd-skip=true` so the systemd xdg-autostart-generator doesn't
wrap it. This is how the **COSMIC** frontend deploys the agent; the
**KDE** frontend ships a `systemd --user` unit
(`PartOf=graphical-session.target`) instead, which Plasma's session
management starts within the correct graphical session.

## The helpers — `sentinel-helper` / `sentinel-helper-kde`

Two interchangeable GUI binaries that paint the dialog —
`sentinel-helper` (COSMIC / libcosmic) and `sentinel-helper-kde` (KDE
Plasma / Kirigami via cxx-qt). They share the CLI contract and wire
output; the backend spawns whichever is installed. Per-spawn:

- Initializes the global Fluent translation bundle from `LANG` /
  `LC_*` (locales embedded at compile time).
- Plays the freedesktop sound cue via `canberra-gtk-play` (silent
  fallback if not installed).
- Decides layer-shell vs xdg-toplevel rendering (auto-falls-back to
  xdg-toplevel on Mutter-based desktops).
- Renders the card; emits `ALLOW` / `DENY` / `TIMEOUT` on stdout
  and exits with the matching code.

Keyboard accessibility:
- Tab / Shift+Tab — cycle Allow / Deny (iced default).
- Enter / Space — activate focused button.
- Escape — always denies (intercepted regardless of focus).
- Allow button is disabled for `min_display_time_ms` after the
  dialog appears, blocking instant scripted clicks.

## Wire formats

### Helper → caller

The helper writes one of `ALLOW\n`, `DENY\n`, `TIMEOUT\n` to stdout
and exits with `0` (Allow) or `1` (Deny / Timeout). The
`sentinel_shared::Outcome` enum is the single source of truth for
the parser.

### Audit log

Lines emitted under syslog identifier `pam_sentinel` or
`sentinel-polkit-agent`, AUTH facility:

```
event=auth.allow source=dialog user=alice service=sudo process=pacman uid=1000 latency_ms=2891 session_type=wayland session_class=user session_remote=0
event=auth.allow source=bypass uid=1000
event=auth.deny  source=dialog user=alice service=sudo process=true uid=1000 latency_ms=12440 …
event=auth.timeout source=agent user=alice action=org.freedesktop.policykit.exec process=pacman …
event=auth.headless reason=no-wayland user=alice service=sudo …
```

Format is logfmt (whitespace-separated `key=value`, values quoted
when necessary). Designed for `journalctl -t pam_sentinel
--output=cat | grep event=auth.deny` to be the SRE-friendly query.

### Bypass channel

System-bus method on `org.sentinel.Agent`:

```
pam_sentinel → agent:  TakeApproval()
agent → pam_sentinel:  true    (approval popped, fast-path the auth)
                       or
                       false   (no approval; fall through to the dialog)
```

## Compatibility matrix

See [README#Compatibility](https://github.com/atayozcan/sentinel#compatibility).
The agent's autostart entry uses `NotShowIn=` to exclude desktops
with built-in polkit agents (GNOME, KDE, XFCE, LXDE, Cinnamon, MATE,
LXQt, Pantheon, Budgie) and lets every other compositor pick it up
automatically.

## Threat model

See [Security policy](./security.md) for the explicit trust
boundaries — what the PAM module trusts vs. doesn't, what the agent
will refuse, supply-chain integrity via Sigstore attestations.
