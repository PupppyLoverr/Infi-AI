#!/usr/bin/env python3
"""Host side of the test VM: GET serves the payload, PUT stores shots/logs.
The VM reaches it at http://10.0.2.2:8088 (QEMU user networking)."""
import http.server, os, sys
root, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=root, **k)
    def do_PUT(self):
        name = os.path.basename(self.path)
        n = int(self.headers.get("Content-Length", 0))
        with open(os.path.join(out, name), "wb") as f: f.write(self.rfile.read(n))
        self.send_response(201); self.end_headers()
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("0.0.0.0", 8088), H).serve_forever()
