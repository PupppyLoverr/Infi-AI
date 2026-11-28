#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
# On the Mac host: create a pty for `tart run --serial-path`, write its name to argv[1] and append
# everything the guest writes to its console to argv[2]. Output survives a guest hang. It ends when the VM has: this
# end of the pty is kept open only until the console's first output (the VM has it open by then), so the read ends
# when the VM closes it. Held for good, it never did, and every run left one of these behind.
import os, sys, tty
m, s = os.openpty()
tty.setraw(s)
with open(sys.argv[1], "w") as f:
    f.write(os.ttyname(s))
out = open(sys.argv[2], "ab", 0)
while True:
    try:
        d = os.read(m, 65536)
    except OSError:
        break
    if not d:
        break
    if s is not None:
        os.close(s)
        s = None
    out.write(d)
