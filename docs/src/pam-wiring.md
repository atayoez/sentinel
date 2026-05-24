# PAM wiring

Sentinel is referenced from the PAM stacks of the services that should trigger
the confirmation dialog. By default the installer wires **polkit-1** and
**`sudo` / `sudo-i` / `su`**; pass `--no-sudo` to wire polkit only.

> **Always test on a fresh install with a second root shell open.** Sentinel
> wires in *prepend-in-place* so a mistake still falls through to a password,
> but `sudo -i` keeps a working privileged shell available regardless.

## Prepend-in-place, not replace

The installer does **not** overwrite your PAM files. For each guarded service
it copies the distro's existing stack — from `/etc/pam.d/<svc>` if present,
otherwise the vendor file under `/usr/lib/pam.d/<svc>` (openSUSE keeps the
shipped files there and lets `/etc/pam.d` shadow them) — and inserts a single
line just before the first `auth … include`:

```
#%PAM-1.0
auth       sufficient pam_sentinel.so   # ← added by Sentinel-KDE
auth       include      common-auth     # ← your distro's real auth (the fallback)
account    include      common-account
password   include      common-password
session    include      common-session
```

Two things this buys you over replacing the file:

- **The real password fallback is preserved.** openSUSE uses `common-auth`
  (not the Fedora-style `system-auth`, which doesn't exist here). Because we
  copy the distro's own stack, the fallback is always the correct one.
- **Leading auth lines are kept.** `su`'s stack starts with
  `auth sufficient pam_rootok.so`; Sentinel's line goes *after* it, so root
  still `su`'s without any prompt.

The `sufficient` control means: if Sentinel returns `PAM_SUCCESS` (you clicked
Allow) the stack is satisfied with no password; any other result continues to
`common-auth`, which prompts.

## Guarding sudo / su (default on)

By default `install.sh` guards `sudo`, `sudo -i`, and `su`. This makes them
**click-to-allow** from a graphical session (no password) and falls back to the
password on a TTY or whenever Sentinel is unavailable.

If you'd rather keep `sudo`/`su` as plain password prompts and let Sentinel
guard only polkit/`pkexec`:

```bash
sudo ./install.sh --no-sudo
```

`sudo-rs` reads the same `/etc/pam.d/sudo` stack, so it's covered identically.

## How `sufficient` interacts with the rest of the stack

`sufficient` is "if this passes we're done; if it fails, keep going". Sentinel
is therefore strictly additive — it never weakens auth, it only ever adds a
click:

| Sentinel returns | Stack behaviour |
|------------------|-----------------|
| `PAM_SUCCESS` (Allow) | Skip rest of `auth`, grant access. |
| `PAM_AUTH_ERR` (Deny / timeout) | Continue to next module → password prompt. |
| `PAM_IGNORE` (disabled / headless / no agent) | Continue to next module → password prompt. |

There's no configuration where Sentinel makes auth *easier* than the
underlying password stack. Worst case it's neutral (you still type your
password); best case (Allow) it's a single click.

## If something breaks

Use the `sudo -i` rescue shell:

```bash
sudo ./uninstall.sh     # reverts every change from the install state file
```

Recovery details:

- Where Sentinel **created** a shadow file (e.g. `/etc/pam.d/sudo` on openSUSE,
  which previously only existed under `/usr/lib/pam.d`), uninstall simply
  deletes it and the vendor stack takes over again.
- Where Sentinel **replaced** an existing `/etc/pam.d/<svc>`, the original is
  saved alongside as `<svc>.pre-sentinel.bak` and restored on uninstall.

Worst case (manual recovery from a TTY): delete the `pam_sentinel.so` line from
`/etc/pam.d/{polkit-1,sudo,sudo-i,su}`, or restore a backup:

```bash
mv /etc/pam.d/polkit-1.pre-sentinel.bak /etc/pam.d/polkit-1   # if a .bak exists
rm  /etc/pam.d/sudo /etc/pam.d/su /etc/pam.d/sudo-i           # shadows → vendor stack returns
```

The transactional state file is `/var/lib/sentinel/install.state`.
