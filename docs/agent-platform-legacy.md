# cosmos-agentd — the agentic layer

Design for spec §6. Slice order chosen so every PR is independently
verifiable on a real boot, cheapest-first, with the Phase-6 demo as the
north star: *an agent opens Files, renames a file via the accessibility
tree (no screenshots), asks approval to delete another, user approves,
then rolls the task back and the file returns.*

## What already exists (reuse, don't rebuild)

- **Compositor IPC** (`cosmos-ipc` on `run/cosmos-compositor.sock`):
  rich verbs — window list/focus/move/resize/snap/close, workspaces,
  mode/tier/accent set, screenshot, quit. This IS the `desktop.*`
  toolset backend.
- **Island card** — the natural home for approvals + agent badges +
  live activities (it already renders clipboard + staged files).
- **Notification daemon** — banners + history + action buttons
  (`NotifyAction` verb already round-trips).
- **btrfs + snapper** — snapshot plumbing landed (PR #97):
  `/etc/default/snapper` `SNAPPER_CONFIGS` includes `root`; `snapper
  create --description … --userdata task=<id>` is the rollback unit.
- **Clipboard** — wlr-data-control ring in the shell; `cosmos-island`
  reads/writes selections.
- **Search-or-Ask launcher** — Ask mode already streams to a default
  agent hook; agentd becomes its backend.

## Milestone slices

### A. `cosmos-agentd` skeleton + MCP server (this PR)
- `cosmos/agentd/` crate: systemd user service binary; Unix socket
  `run/cosmos-agentd.sock` speaking newline-delimited JSON-RPC (MCP
  `tools/list` + `tools/call` per spec — real MCP wire format,
  stdio-variant for agents that can't socket).
- Tool surface v1 (all real, zero mocks):
  - `desktop.windows/list|focus|move|snap|close`, `desktop.workspaces`,
    `desktop.launch` — thin proxies over cosmos-ipc (semantic first).
  - `desktop.screenshot` — permission-gated; returns a PNG path under
    `~/Agents/<name>/shots/` via IPC `screenshot`.
  - `clipboard.read|write` — via the shell clipboard socket.
  - `notify.send` — via the notification socket.
  - `files.read|write|search|move` — scoped to policy roots
    (`~` read, `~/Agents/<name>/` + approved dirs write).
  - `system.settings.get|set` — `~/.config/cosmos/` files.
  - `audit.log` — every call appended to
    `~/.local/share/cosmos/agent-audit/<date>.jsonl` (6.8).
- `cosmos-agent.toml` policy file format (6.3): agent name, allowed
  read/write dirs, network domains, tool allowlist. Default policy:
  read `~`, write `~/Agents/<name>/`, no net.

### B. Agent identities + sandboxed launch (6.3)
- `cosmos-agent-run <name>` helper: setpriv to `cosmos-agent-<name>`
  uid (users created at install), `bwrap` with policy-mapped binds,
  Landlock fs rules where the kernel allows (QEMU: degrade to bwrap
  only, logged).
- agentd spawns agents under it and proxies their MCP traffic so the
  sandbox never needs the compositor socket.

### C. Approvals + island integration (6.5/6.6)
- Policy `sensitive` flag on tools (files.delete, system.settings.set,
  desktop.screenshot, package ops): call → pending state → approval
  card in the island (command/diff shown, Allow once / Always / Deny)
  → result streamed back to the agent. Super+Esc pauses all agents
  (agentd global toggle — also the Control Centre AGENTS switch).

### D. Headless seat + peek (6.4)
- agentd runs a nested cosmos-compositor instance on a private
  wayland socket; the agent's windows live there. `desktop.seat_peek`
  returns periodic frames to a peek surface in the island (readback →
  shm — we already have the capture path). Take-over = compositor
  forwards seat input to the nested seat.

### E. Rollback + audit UI (6.7/6.9)
- Pre-task `snapper create` per affected subvolume; Agents app
  timeline lists tasks with "Roll back this task" → `snapper undochange`.
- Audit log already written in slice A — the app renders it.

### F. Onboarding + credentials (6.1)
- First-boot page in the greeter/Settings: pick agent (opencode ships
  in-image; claude/codex/gemini via npx; ollama if RAM allows;
  OpenAI-compatible endpoint). Keys into Secret Service via
  `secret-tool`; never a file.

### G. Skills folder (6.11) + built-in moments (6.10)
- `/usr/share/cosmos/skills/*.md` — how CosmosOS works (config paths,
  theming, widget API, MCP tool list).
- Crash-notification "Diagnose with agent" action, file context-menu
  agent verbs, "make me a widget" from Ask.

## Non-goals (explicit)

- No local-LLM requirement in-image (RAM budget forbids it; ollama is
  opt-in only).
- No raw input injection tool — a11y/semantic first, screenshots last
  (spec rule). AT-SPI tree access arrives with slice B/C once we
  validate pyatspi coverage on uitk apps.
