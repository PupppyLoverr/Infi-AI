# CosmosOS security posture

Threat model: **single-user, kiosk-style live system**. One physical seat, one
user (`cosmos`, uid 1000), boots straight to the desktop. There is **no
intended remote attack surface** — no remote login services are installed, and
nothing should listen on a non-loopback interface. Hardening decisions optimize
for a usable appliance, not a multi-user host.

Audited against the image built at main `910d3ab` + opencode (devin/opencode).
`ss`/`pkexec` were absent at audit time; `iproute2` was added by this pass.

## Remote / login surface

| Check | State |
|---|---|
| sshd / dropbear / xrdp / vnc / telnet | **not installed** (only `libssh2-1t64`, a client lib) |
| gettys | `getty@tty1` only, with the `agetty --autologin cosmos` drop-in — by design |
| serial console | kernel logs to ttyS0 but **no getty** — no login path |
| TCP/UDP listeners | resolved DNS stub on `127.0.0.53`/`127.0.0.54` (loopback) only; NM, dbus, pipewire, journald use unix sockets |
| LLMNR (5355) | **was bound on `0.0.0.0`/`::`** — systemd-resolved default; disabled via `resolved.conf.d/no-llmnr.conf` (`LLMNR=no`, `MulticastDNS=no`) |
| Enabled services | NM, networkd, resolved, upower + stock systemd units (pcrlock, sysext, pstore, network-generator) — nothing that binds an external socket |

## Privilege model

| Check | State |
|---|---|
| Session user | `cosmos` uid 1000, groups `tty video input render netdev` — non-root |
| `sudo` | installed; `cosmos` has `NOPASSWD: ALL` (`/etc/sudoers.d/cosmos`) — the account password is locked so a password prompt could never be answered; passwordless sudo is the kiosk-model trade-off that makes `apt`/`dpkg` installs usable. Still acceptable under the threat model: single-user, no remote shell, and a local user who can run `sudo` already owns the box. |
| `pkexec` | **not installed** — no polkit-based shell escalation path |
| Passwords | `root:*` and `cosmos:!` in /etc/shadow — both locked; `su`/password logins are dead ends |
| suid binaries | stock set + `sudo`/`sudoedit` (added with the sudo package): `chfn chsh gpasswd mount newgrp passwd su umount`, `dbus-daemon-launch-helper`, `polkit-agent-helper-1` |
| polkit | `polkitd` active; `/etc/polkit-1/rules.d/60-cosmos-nm.rules` grants `netdev` (which contains cosmos) all `org.freedesktop.NetworkManager.*` actions — required because the autologin seat isn't reliably "active" to polkit. Nothing else is granted. |

## Kernel / sysctl (90-cosmos.conf)

| Setting | Value | Note |
|---|---|---|
| `kernel.dmesg_restrict` | 1 | already the live default; pinned |
| `kernel.kptr_restrict` | **2** (was 0) | kernel pointers never leak via /proc |
| `fs.protected_hardlinks` / `symlinks` | 1 / 1 | already live; pinned |
| `fs.protected_fifos` / `regular` | **2 / 2** (was 0) | no fifo/regular-file follow tricks in sticky dirs |
| `net.ipv4.conf.*.rp_filter` | **1** (was 0) | drop spoofed-source packets |
| `net.ipv4.tcp_syncookies` | 1 | already live; pinned |
| `kernel.unprivileged_bpf_disabled` | 2 | already live (hard-disable); pinned |
| umask | 0022 | verified live; stock `login` default, HOME_MODE 0700 |
| World-writable dirs | none non-sticky (`/tmp`, `/var/tmp`, `/dev/shm` all 1777) |

## Changes made in this pass

- `image/build.sh`: `iproute2` added to PACKAGES (`ss`/`ip` for audit + user
  debugging); `etc/sysctl.d/90-cosmos.conf` drop-in pins the table above
  (actual deltas: `kptr_restrict 0→2`, `rp_filter 0→1`,
  `protected_fifos 0→2`, `protected_regular 0→2`);
  `etc/systemd/resolved.conf.d/no-llmnr.conf` disables LLMNR/mDNS (was
  listening on `0.0.0.0:5355` — found by the audit, the one real external
  listener).
- Overlay files are `chown -R root:root` at staging (were uid-1000-owned in
  the guest).

## Accepted posture / known gaps

- **Autologin with no lock screen** is the product. Physical access = the
  session; there is no lock/unlock concept yet.
- **No disk encryption** — it's a live-style image; contents are public.
- **No firewall ruleset** — acceptable because nothing listens externally;
  revisit if any service is ever added that binds beyond loopback.
- **opencode** is a local CLI tool; it only networks if the user configures a
  provider/model. It runs unprivileged as `cosmos`.
- **NetworkManager polkit grant is broad** for the `netdev` group by design
  (kiosk convenience) — documented here as the single deliberate privilege
  relaxation.
