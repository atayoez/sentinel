# Troubleshooting

## "I can't `sudo` / `pkexec`" — recovery

From your second root shell (`sudo -i`):

```bash
./uninstall.sh     # reverts everything from /var/lib/sentinel/install.state
```

Because Sentinel wires in *prepend-in-place* (`auth sufficient` on top of the
distro stack), a broken module already falls through to your password — a true
lockout shouldn't happen. Worst case, from a TTY (Ctrl+Alt+F3) as root:

```bash
# remove the Sentinel line, or restore a backup if one exists:
rm /etc/pam.d/sudo /etc/pam.d/su /etc/pam.d/sudo-i   # openSUSE: shadows → vendor stack returns
mv /etc/pam.d/polkit-1.pre-sentinel.bak /etc/pam.d/polkit-1   # if a .bak exists
rm /usr/lib64/security/pam_sentinel.so
```

## `pkexec`/`sudo` still asks for a password (the bypass isn't firing)

Work down this list — most issues live here.

**1. Is the agent registered and on the bus?**
```bash
systemctl --user is-active sentinel-polkit-agent.service
journalctl --user -t sentinel-polkit-agent --since "5 min ago" --no-pager
busctl --system status org.sentinel.Agent     # should show UnixUser = your uid
```
You should see `registered as polkit auth agent` shortly after login. If
another agent won the race, make sure `plasma-polkit-agent.service` is masked
(the installer does this):
```bash
systemctl --user mask --now plasma-polkit-agent.service
systemctl --user restart sentinel-polkit-agent.service
```

**2. Did the bypass fire?** After clicking Allow:
```bash
journalctl --user -t sentinel-polkit-agent --since "1 min ago" | grep agent.bypass
```
- `event=auth.allow source=agent.bypass` → it worked.
- `event=auth.error source=agent.helper1 …` → the PAM module didn't approve;
  continue below.

**3. SELinux.** Sentinel's bypass is designed to work under enforcing SELinux,
but confirm it's the standard policy:
```bash
cat /sys/fs/selinux/enforce          # 1 = enforcing
# quick sanity check (temporary!):
sudo setenforce 0; pkexec true; sudo setenforce 1
```
If it works no-password only under `setenforce 0`, your SELinux policy is
denying the D-Bus call — capture it with `sudo ausearch -m avc -ts recent` and
file an issue. (The shipped design needs no custom policy on stock Tumbleweed.)

**4. PAM module loaded?** It must be in the multilib dir:
```bash
ls -l /usr/lib64/security/pam_sentinel.so
grep pam_sentinel /etc/pam.d/polkit-1
```

## The dialog never appears

Check the agent is alive (above). If it's running but no dialog shows, the
compositor may lack `zwlr-layer-shell-v1`; force the windowed fallback to test:
```bash
sentinel-helper-kde --windowed --title test --message hi
```

## No sound

The cue tries `canberra-gtk-play` first, then `pw-play`/`paplay`/`ffplay`. For
the theme-aware path, install canberra:
```bash
sudo zypper install canberra-gtk-play
```
Otherwise the PipeWire fallback is used (present on any Plasma desktop). Check
the configured cue in `/etc/security/sentinel.conf` (`[audio] sound_name`); set
it to `""` to disable.

## `pkexec` prints "Not authorized. This incident has been reported."

That's pkexec's standard message after a failed auth — **including a clean Deny
click**. The "incident reported" line is hardcoded in pkexec(1); polkit's
protocol doesn't distinguish "user declined" from "auth failed", so the agent
can't suppress it.

## More verbose logs

The agent takes `--debug` (dumps `details[…]` from every
`BeginAuthentication`):
```bash
systemctl --user stop sentinel-polkit-agent.service
/usr/lib/sentinel-polkit-agent --debug      # Ctrl-C when done, then:
systemctl --user start sentinel-polkit-agent.service
```
All auth events from the last few minutes:
```bash
journalctl --user -t sentinel-polkit-agent --since "5 min ago" --no-pager | grep event=auth
```

## Reporting bugs

Open an issue at <https://github.com/atayozcan/sentinel-kde/issues>. For
security issues use private vulnerability reporting — see
[Security policy](./security.md).
