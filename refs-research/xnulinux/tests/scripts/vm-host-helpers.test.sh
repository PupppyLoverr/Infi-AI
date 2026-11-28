#!/bin/bash
# SPDX-License-Identifier: GPL-2.0-only
# The Mac-side helpers of the Linux VM. scripts/vm.sh: without the session files it says which ones it needs and runs
# nothing; with them it reaches the VM also from a directory whose path has a space. scripts/serial-pty.py: the console
# logger of vm-run-serial.sh ends when the VM has closed the console.
. "$(dirname "$0")/lib.sh"
sandbox_repo
for c in ssh scp; do stub $c <<EOF	# the stand-in's arguments, one a line
printf '%s\n' "\$@" > "\$T/$c.args"
EOF
done
has() { grep -qxF -- "$2" "$T/$1.args"; }	# has ssh|scp ARGUMENT: the stand-in was given exactly that argument

# vm.sh with nothing beside the repository
out=$(bash "$T/repo/scripts/vm.sh" uname -a 2>&1); st=$?
check 'vm.sh without ../vm.ip and ../vmkey: fails' [ $st != 0 ]
check '... names both files and where they are' sh -c 'echo "$1" | grep -q "$2/vm.ip" && echo "$1" | grep -q "$2/vmkey"' - "$out" "$T"
check '... says how to get the address' sh -c 'echo "$1" | grep -q "tart ip"' - "$out"
check '... and has not called ssh' not called '^ssh'

# with them, in a directory whose path has a space
D="$T/my work"; mkdir -p "$D"; cp -Rp "$T/repo" "$D/repo"; echo 192.168.64.7 > "$D/vm.ip"; : > "$D/vmkey"
bash "$D/repo/scripts/vm.sh" uname -a >/dev/null 2>&1
check 'vm.sh from a path with a space: ssh is called' called '^ssh'
check '... with the key as one argument' has ssh "$D/vmkey"
check '... on port 22 without ../vm.port' has ssh Port=22
check '... to the address in ../vm.ip' has ssh admin@192.168.64.7
check '... with the command' has ssh uname
echo 2223 > "$D/vm.port"; echo 127.0.0.1 > "$D/vm.ip"
bash "$D/repo/scripts/vm.sh" -put "$D/a file" /var/tmp/there >/dev/null 2>&1
check 'vm.sh -put under krunkit: scp on the port in ../vm.port' has scp Port=2223
check '... with the key as one argument' has scp "$D/vmkey"
check '... the file to copy as one argument' has scp "$D/a file"
check '... to the VM' has scp admin@127.0.0.1:/var/tmp/there

# serial-pty.py: a stand-in VM opens the pty, writes its console output and ends
python3 "$T/repo/scripts/serial-pty.py" "$T/serial-name" "$T/serial-cap.log" & logger=$!
check 'serial-pty.py: writes the name of its pty' wait_for 5 test -s "$T/serial-name"
python3 -c 'import os, sys, time
fd = os.open(sys.argv[1], os.O_RDWR | os.O_NOCTTY)
for i in range(3):
    os.write(fd, b"console line %d\n" % i); time.sleep(0.1)
os.close(fd)' "$(cat "$T/serial-name")"
check '... the console output is in the log' wait_for 5 grep -q 'console line 2' "$T/serial-cap.log"
check '... and it ends when the VM has closed the console' wait_for 5 not kill -0 $logger
finish
