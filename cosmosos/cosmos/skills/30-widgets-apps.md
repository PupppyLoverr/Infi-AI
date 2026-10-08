# Widgets, apps, and "make me a …"

The Start panel's WIDGETS strip renders agent-generated widgets
alongside the system ones. A widget is a directory in
`~/.local/share/cosmos/widgets/<name>/` containing:

- `widget.toml` — `title = "…"`, `icon = "cpu"|"disk"|"clock"`,
  `refresh_secs = 30` (clamped 5–3600).
- `data.sh` — a /bin/sh script run once per refresh; prints
  `big=`, `small=`, `bar=0-1` key=value lines on stdout.

The shell caches output between refreshes; scripts get a 2s wall-clock
cap (SIGKILL after) so a hang never stalls the panel, and a failing
script is skipped — never rendered blank. Keep them cheap: no daemons,
no sockets — the RAM budget applies to you too.

## Apps

`.desktop` files in `~/.local/share/applications/` appear in the
launcher and dock. To package something for the user: build it inside
your write root, write the desktop entry pointing at it, and surface a
notification asking the user to pin it.
