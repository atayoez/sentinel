# Architecture

Sentinel is three binaries plus one shared crate, in a single Cargo workspace.

```
crates/
├── sentinel-shared/        # config schema, /proc + logind readers, Outcome wire
│                           # enum, log_kv helpers, D-Bus name/path constants,
│                           # POLKIT_PAM_SERVICE, audit::init_syslog
├── pam-sentinel/           # cdylib → pam_sentinel.so   (the D-Bus bypass client)
├── sentinel-polkit-agent/  # bin → the polkit agent + the org.sentinel.Agent service
└── sentinel-helper-kde/    # bin → the Kirigami/Breeze dialog (Rust + cxx-qt)
```

The PAM module installs to the distro's PAM module dir — `/usr/lib64/security`
on openSUSE/Fedora (multilib), detected at install time from `pam_unix.so`.

## The PAM module — `pam_sentinel.so`

Loaded by libpam on every authentication attempt for whatever services have it
wired in. For each call it picks one of:

- **Bypass:** the agent has already pre-approved this auth. `pam_sentinel`
  asks the agent over the **system D-Bus** and, on success, returns
  `PAM_SUCCESS` immediately (no password). See *The bypass* below.
- **Headless:** no Wayland display for the requesting user → return whatever
  `headless_action` says (default `PAM_IGNORE` → password prompt).
- **Disabled:** `enabled = false` in config for this service → `PAM_IGNORE`.
- Otherwise it falls through to the rest of the PAM stack (the password).

> The agent owns the dialog. `pam_sentinel` itself doesn't spawn the GUI in
> the polkit path — it only consults the agent's pre-approval. (The module
> retains a direct-dialog path for non-agent PAM services.)

Identifying the requesting user uses `/proc/<ppid>/loginuid` (set by PAM at
login, inherited through forks, immune to setuid), falling back to
`/proc/<ppid>/status` `Uid:`, then `getuid()`. In the polkit-helper path the
authoritative answer is `PAM_USER`.

## The polkit agent — `sentinel-polkit-agent`

A per-user agent that registers with polkitd as the session's
`org.freedesktop.PolicyKit1.AuthenticationAgent`. On `BeginAuthentication` it
shows the dialog (via `sentinel-helper-kde`), and on **Allow** queues a
one-shot approval, then drives `polkit-agent-helper-1` to satisfy polkit's
cookie validation — which runs the polkit-1 PAM stack, where `pam_sentinel.so`
consumes the approval.

### Runs as a systemd *user* service

On Plasma 6 the agent **must** be a `systemd --user` service
(`packaging/systemd/user/sentinel-polkit-agent.service`,
`PartOf=graphical-session.target`) to register cleanly with polkitd. A
hand-spawned / XDG-autostart agent fails polkit's session-equality check. The
installer masks `plasma-polkit-agent.service` so Sentinel is the sole agent.

### The bypass — over the system D-Bus

On **Allow** the agent claims `org.sentinel.Agent` on the **system bus** and
serves a single method, `TakeApproval`, which pops one non-expired approval
from an in-memory queue (one-shot, 1 s TTL; `CancelAuthentication` drains it so
a stale approval can't be claimed by a racing auth).

`pam_sentinel.so` (running as root inside `polkit-agent-helper-1`):

1. Resolves the uid being authenticated (`PAM_USER`).
2. Verifies `org.sentinel.Agent`'s **owner uid == that uid** via
   `GetConnectionUnixUser` — so a same-name squatter from another uid can't
   forge an approval.
3. Calls `TakeApproval`; on `true`, returns `PAM_SUCCESS`.

The D-Bus policy (`packaging/dbus/org.sentinel.Agent.conf`) lets any user own
the name but restricts the method to **root** callers, so a non-root process
can't drain the queue.

#### Why D-Bus instead of a unix socket

polkit 121+ forks `polkit-agent-helper-1` from polkitd, so on SELinux systems
(openSUSE Tumbleweed) the helper runs as `policykit_t`. SELinux **denies**
`policykit_t` writing an arbitrary `var_run_t` socket (`sesearch` confirms it
has no `sock_file write`), which defeats *any* `/run` socket — but it
**already allows** `policykit_t userdomain:dbus send_msg` (the polkit agent
protocol itself, and exactly how `pam_fprintd` does passwordless auth). So the
D-Bus channel rides existing MAC policy: it works under **enforcing SELinux**
with no custom policy module and no `polkit.service` sandbox override.

### Identity selection

`unix-user` identities are preferred over groups; the uid matching the agent's
own uid wins; the first non-root `unix-user` is the fallback. The installer's
polkit admin rule (`/etc/polkit-1/rules.d/49-sentinel-admin.rules`) makes the
logged-in user a polkit administrator so `auth_admin` actions authenticate the
user (`PAM_USER` = the user), not root. See
`crates/sentinel-polkit-agent/src/identity.rs`.

## The helper — `sentinel-helper-kde`

A Qt/QML (cxx-qt + Kirigami + Breeze) binary that paints the dialog. Per spawn:

- Plays the freedesktop sound cue, detached so it survives the dialog's exit —
  `canberra-gtk-play` first, then `pw-play`/`paplay`/`ffplay`/`aplay`.
- Renders the card as a `zwlr-layer-shell-v1` overlay (KWin) via the installed
  `org.kde.layershell` QML plugin, with an xdg-toplevel (`--windowed`)
  fallback. QML is embedded in the binary as a **qrc** (tamper-proof).
- Emits `ALLOW` / `DENY` / `TIMEOUT` on stdout and exits 0 / 1.

Hardening: every controller-supplied string forces `Text.PlainText` (the
requesting process's exe/cmdline/cwd are attacker-influenceable; AutoText would
render injected markup), `/proc` fields are length-clipped, and Allow is
disabled for `min_display_time_ms` to block instant scripted clicks.

## PAM wiring — prepend-in-place

The installer doesn't replace PAM files. For each guarded service (`polkit-1`,
and by default `sudo`/`sudo-i`/`su`) it copies the distro's existing stack —
from `/etc/pam.d` if present, else the vendor `/usr/lib/pam.d` — and inserts
`auth sufficient pam_sentinel.so` just before the first `auth … include`. This:

- keeps the distro's real password fallback (openSUSE uses `common-auth`, not
  the nonexistent `system-auth`);
- preserves leading lines like `su`'s `pam_rootok.so` (root still skips);
- makes uninstall trivial where `/etc` shadows the vendor file (delete our copy
  and the vendor stack returns).

## Wire formats

### Helper → caller

`ALLOW\n` / `DENY\n` / `TIMEOUT\n` on stdout, exit `0` (Allow) or `1`
(Deny/Timeout). `sentinel_shared::Outcome` is the single source of truth.

### Bypass — D-Bus

```
org.sentinel.Agent  (system bus)
  /org/sentinel/Agent
  org.sentinel.Agent.TakeApproval() -> b   # true = pre-approved (consume it)
```
Caller is restricted to root by the bus policy; `pam_sentinel` verifies the
name owner's uid before trusting the reply.

### Audit log

logfmt under syslog identifiers `pam_sentinel` / `sentinel-polkit-agent`, AUTH
facility:

```
event=auth.allow source=agent user=alice action=org.freedesktop.policykit.exec process=pkexec latency_ms=2891 …
event=auth.allow source=agent.bypass action=org.freedesktop.policykit.exec
event=auth.deny  source=agent user=alice action=… process=true latency_ms=12440 …
event=auth.headless reason=no-wayland user=alice service=sudo …
```

`journalctl -t sentinel-polkit-agent --output=cat | grep event=auth` is the
SRE-friendly query. (Note: inside `polkit-agent-helper-1` the helper's sandbox
masks `/dev/log`, so `pam_sentinel`'s own bypass line may not appear there —
the agent's `source=agent.bypass` line is the authoritative record.)

## Threat model

See [Security policy](./security.md) for the explicit trust boundaries — what
the PAM module trusts, what the agent refuses, and the SELinux posture.
