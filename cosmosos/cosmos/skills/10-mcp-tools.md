# The OS as MCP tools (cosmos-agentd)

Transport: MCP over stdio (`cosmos-agentd --stdio`) or the session
socket `$XDG_RUNTIME_DIR/cosmos-agentd.sock`. Wire format is
newline-delimited JSON-RPC: `initialize`, `tools/list`, `tools/call`,
`ping`. Pass your agent name in `initialize` → `params.clientInfo.name`;
it selects your policy file.

## Tool surface

- `desktop.windows.list|focus|close` — real window table; ids are
  stable for the window's life.
- `desktop.windows.snap {id, zone}` — zones from the shared snap
  layouts (left, right, top, bottom, quarter-*, third-*, free-half…).
- `desktop.windows.move_workspace {id, workspace}` /
  `desktop.workspaces.list|switch {workspace}` — 0-based workspaces.
- `desktop.launch {command}` — spawn a desktop app.
- `desktop.screenshot {name}` — PNG to `~/Agents/<you>/shots/`.
  Permission-gated: prefer semantic data (window trees, a11y) first;
  screenshot is the LAST resort per OS policy.
- `system.settings.get [key]` / `system.settings.set {key, value}` —
  reads/writes the compositor-persisted config (dark mode, accent,
  dock side, tier, tiling default…). Hot-reloads live.
- `files.read {path}` / `files.write {path, content}` /
  `files.search {root, query}` / `files.move {from, to}` —
  real fs ops, scoped by your policy roots.

## Your policy file

`~/.config/cosmos/agents/<name>.toml`:

```toml
read_roots  = ["/home/cosmos"]
write_roots = ["/home/cosmos/Agents/opencode"]
tools       = []            # empty = all tools
sensitive   = ["files.delete"]  # needs the user's approval card
```

Default (no file): read `~`, write `~/Agents/<name>/`, all tools.
Anything outside policy fails closed with an explanatory error — don't
retry in a loop; report the block to the user.

## Audit

Every call is appended to `~/.local/share/cosmos/agent-audit/
day-<n>.jsonl` (who, tool, ok, detail) — the user can read it in the
Agents app. Expect your actions to be visible.
