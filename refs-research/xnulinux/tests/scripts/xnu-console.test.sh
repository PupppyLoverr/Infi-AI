#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# scripts/xnu-console.sh against a stand-in VM booted the way scripts/xnulinux-boot.sh boots it: the hosted console's
# lines are in the kernel log (pr_info, read through journald) and not on the VM console (`dmesg -n`), so the captured
# serial log has none of them. The script and scripts/vm.sh run as they are; ssh runs its command in the stand-in VM,
# whose journalctl follows a file and whose "hosted console shell" answers each line that arrives in console-in.
. "$(dirname "$0")/lib.sh"
sandbox_repo
echo 127.0.0.1 > "$T/vm.ip"; : > "$T/vmkey"	# what scripts/vm.sh reads next to the repository
echo '[    1.000000] systemd[1]: Reached target multi-user.target' > "$T/serial-cap.log"	# scripts/vm-run-serial.sh's capture
: > "$VM/journal"; : > "$VM/var/tmp/console-in"
stub_ssh
vmstub sudo <<'EOF'
exec "$@"
EOF
vmstub journalctl <<'EOF'
# (ssh passes its own input on: started with the script's, here a FIFO, it would take what is typed)
[ ! -p /dev/stdin ] || { echo "the log's reader was started with the script's own input" >&2; exit 1; }
case " $* " in *" -k "*) ;; *) echo "journalctl (stand-in): not the kernel log: $*" >&2; exit 1;; esac
case " $* " in *" -f "*" -o cat "*|*" -o cat "*" -f "*) exec tail -n 0 -F "$VM/journal";; esac	# -o cat: the message alone
echo "journalctl (stand-in): only -f with -o cat: $*" >&2; exit 1
EOF
tail -n 0 -F "$VM/var/tmp/console-in" 2>/dev/null | while IFS= read -r l; do	# the hosted system
  printf '%s\n' 'fki: xnu: a kernel line that is not the console' "fki: darwin console: xnulinux# $l" \
    "fki: darwin console: reply to $l" >> "$VM/journal"
done &

mkfifo "$T/in"
/bin/bash "$T/repo/scripts/xnu-console.sh" < "$T/in" > "$T/out" 2>&1 & pid=$!
exec 3> "$T/in"
wait_for 3 called '^journalctl '; sleep 0.5	# the reader has had time to start, as a person types later than that
echo 'uname -a' >&3
check 'a line typed arrives in the console input of the hosted system' wait_for 5 grep -qx 'uname -a' "$VM/var/tmp/console-in"
check 'what the console prints back is shown, while the console is otherwise quiet' wait_for 5 grep -qx 'reply to uname -a' "$T/out"
echo 'second' >&3
check '... for the next line too' wait_for 5 grep -qx 'reply to second' "$T/out"
check '... and nothing but the console lines' not grep -q 'not the console' "$T/out"
kids=$(pgrep -P $pid | tr '\n' ' ')
exec 3>&-	# end of input (^D)
check 'the script ends with its input' wait_for 5 not kill -0 $pid
sleep 0.3; left=
for k in $kids; do kill -0 "$k" 2>/dev/null && left="$left [$(ps -o command= -p "$k" | sed "s|$T|\$T|g")]"; done
check "... and leaves none of its processes running${left:+: left$left}" [ -z "$left" ]
[ $FAILED = 0 ] || sed 's/^/    | /' "$T/out"
finish
