# Security policy

## Reporting a vulnerability

1. **Preferred:** [GitHub Private Vulnerability Reporting](https://github.com/atayozcan/sentinel-kde/security)
   ("Report a vulnerability").
2. **Email:** `atay@oezcan.me`, subject "Sentinel security".

Please don't open public issues for security bugs.

## Threat model summary

Sentinel has two trust boundaries.

### 1. The PAM module (`pam_sentinel.so`)

Runs in-process of whatever privileged binary's PAM stack references it —
`polkit-agent-helper-1`, `sudo`, `su`. It trusts libpam, the root-owned
`/etc/security/sentinel.conf`, and the kernel's `/proc/<pid>/loginuid`. It does
**not** trust the host binary's environment (locale-relevant variables, when
used, are recovered from the requesting user's `/proc/<pid>/environ` against a
strict allowlist).

It is **fail-open**: any error talking to the agent (no agent, wrong owner,
refused, malformed) returns `None`, and the PAM stack falls through to the
normal password. A broken Sentinel never blocks legitimate auth.

### 2. The polkit agent + the bypass channel

The agent runs as the user and exposes `org.sentinel.Agent` on the **system
D-Bus** with one method, `TakeApproval`. The bypass is hardened on both ends:

- **Only root may call it.** The D-Bus policy
  (`packaging/dbus/org.sentinel.Agent.conf`) restricts `send_destination` to
  root, so a non-root local process can't drain the one-shot approval queue.
- **The PAM module verifies the responder.** Before trusting a reply,
  `pam_sentinel` checks via `GetConnectionUnixUser` that `org.sentinel.Agent`
  is owned by the **uid it is authenticating**. A same-name squatter running as
  a different uid is rejected.
- Approvals are **one-shot** and expire after 1 second;
  `CancelAuthentication` drains the queue so a stale approval can't be claimed
  by a racing auth.

### SELinux posture

The bypass deliberately uses D-Bus rather than a socket so it rides existing
MAC policy. Under enforcing SELinux (openSUSE Tumbleweed), the polkit helper
runs as `policykit_t`, which is **denied** writing an arbitrary socket but is
**already allowed** `dbus send_msg` to user domains (the polkit agent protocol;
the same path `pam_fprintd` uses). So Sentinel needs **no custom SELinux policy
module** and **no weakening of polkit's sandbox** — `polkit.service` keeps its
full vendor hardening.

## Out of scope

- **Same-uid attacks.** A process running as your user can drive polkit
  directly; Sentinel is a UI confirmation, not a sandbox.
- Compositor / kernel issues themselves.
- Issues in upstream sudo, polkit, `pam_unix`, `polkit-agent-helper-1`.

When in doubt, send the report anyway and we'll triage.
