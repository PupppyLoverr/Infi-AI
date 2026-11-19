# Agent task protocol (panel ↔ runtime)

How `solyra-panel` shows agent work and records the human's decisions. The panel
implements the reader and the writers; **no agent runtime exists yet** (see
`agent-platform.md`), so today this is a contract plus fixture-tested code in
`shell/panel/src/agent.rs`.

Nothing in the panel simulates an agent. With no runtime directory the UI says
"No agent runtime is connected".

## Honesty rules the UI follows

- A decision written by the panel is a **message to the runtime**. It is not a
  sandbox and enforces nothing. The approval card says so.
- The panel shows only what events report. A missing `environment`,
  `resource` or `reversible` field renders as "not reported by the agent",
  never as a default such as "sandboxed" or "reversible".
- Event text is the agent's own words, not independently verified evidence.
- The panel never creates the runtime directory. Its absence is the "no
  runtime" signal.

## Version 2: questions, messages, status

v2 adds what the notch needs to talk to agents. Everything in v1 still holds.
The runtime pieces live in `agent/` (`solyra-agent`).

- `question` event: `request_id`, `text`, `choices?` (up to 8). Shown as a
  question; answered with a decision line `{"decision":"answer","text":"..."}`.
  The runtime closes it with `question_resolved`.
- `status` event: `state` is `working` or `idle`; `detected:true` marks a
  guess from terminal activity rather than the agent's own report. It never
  overrides an open question, a blocker, or a finished/failed task.
- `started` may carry `agent` (the program's name) and `interactive: true`,
  meaning the agent runs under `solyra-agent wrap` and can receive text.
- `messages/<id>.jsonl`: lines `{"ts","text","source"}` written by the panel;
  `wrap` types each into the agent's terminal followed by Enter. Text is
  stripped of control characters and capped at 2000 characters by the sender.
- `grants/<id>.txt`: tools approved "for this task" (hook-side, one per line).

### Hooks

`solyra-agent hook claude` reads Claude Code's hook JSON on stdin.
`PermissionRequest` becomes an `approval_request` and the hook **waits for the
panel** (default 120 s), then prints Claude's allow/deny decision. If nobody
answers, or no panel is running, it prints nothing and Claude asks in the
terminal as usual: the hook never approves on its own. `solyra-agent
install-hooks claude` shows and, with `--apply`, writes the hook entries into
`~/.claude/settings.json` (backup kept, idempotent). Codex's `notify` hook is
supported for status only. These contracts follow the tools' documentation and
are not verified against live installs in CI.

## Layout

```text
$XDG_RUNTIME_DIR/solyra-agent/
  tasks/<id>.jsonl        runtime appends events
  decisions/<id>.jsonl    panel appends approvals/denials
  requests/<ts>-<n>.json  panel writes draft task requests (launcher)
```

`<id>` matches `[A-Za-z0-9_-]{1,64}`. Files with other names are ignored and
reported on stderr.

## Events (`tasks/<id>.jsonl`, one JSON object per line)

Every event has `type` and `ts` (Unix seconds).

| `type` | Fields | Effect |
| --- | --- | --- |
| `started` | `name`, `objective?`, `environment?` | Names the task. `environment` is shown verbatim under "Runs in". |
| `milestone` | `text` | A short step the agent reached. Clears `blocked`. |
| `approval_request` | `request_id`, `action`, `resource?`, `reason?`, `reversible?` (bool), `scopes?` (`"once"`, `"task"`) | Status becomes *Needs approval*. `scopes` defaults to `["once"]`. |
| `approval_resolved` | `request_id`, `decision?` | The runtime acknowledges an answer; the card clears. |
| `blocked` | `text` | Status *Blocked*; `text` is shown as the blocker. |
| `finished` | `summary?` | Terminal. |
| `failed` | `error?` | Terminal. |

Reader behaviour: an unterminated last line is a write in progress and is
ignored; other unparseable lines are counted and surfaced ("N events could not
be read"); unknown `type`s are counted, not hidden. Text is flattened, control
characters become spaces and fields are clipped to 400 characters. Only the last
2 MiB of a log is read. A live task with no event for 120 s is labelled *quiet*.
Finished/failed tasks drop off the list after 6 h.

## Decisions (`decisions/<id>.jsonl`)

```json
{"ts":1700000000,"request_id":"r1","decision":"approve","scope":"task","source":"solyra-panel"}
```

`decision` is `approve` or `deny`; `scope` is `once` or `task` (only offered when
the request listed it). Each decision is a single appended line. If the write
fails the card shows "Decision not recorded: …" and does not pretend otherwise.
Until the runtime sends `approval_resolved`, the card shows "Sent … waiting for
the runtime".

## Requests (`requests/<ts>-<n>.json`)

```json
{"ts":1700000000,"objective":"sort my downloads","source":"launcher"}
```

Written when the user chooses the launcher's **Ask an agent** row. It is a draft:
the runtime decides whether and how to act, and must ask for clarification or
approval itself.

## Keys

In the agent card: Up/Down select a task, **A** approve once, **T** approve for
this task, **D** deny, **Esc** closes the card without answering. Enter is
deliberately unbound. `solyra-panel toggle-agents` and `Mod+Shift+A` open it.
