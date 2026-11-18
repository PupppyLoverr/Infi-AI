# Agent platform (P5)

Solyra is a desktop for humans and agents. That means agents get **their own
space, their own identity, and a visible cost** — not a chat box bolted onto a
terminal. The design below is what P5 builds; nothing here is implemented yet.

The previous implementation (`docs/agent-platform-legacy.md`, CosmosOS v1–v6)
had the right shape: a policy file, approvals rendered in the shell, a JSONL
audit trail, and an MCP tool surface. That design carries over. What changes is
the platform underneath it: `zeron` as the control plane, niri workspaces for
Agent Space, and bootc for OS-level rollback.

## Agent Space

Agents work in a workspace set the human is not looking at.

- Workspaces 7–9 are reserved for agent sessions by default; a session gets one
  workspace and its windows live there.
- Windows owned by an agent are marked: a 2 px accent border in the agent's
  colour and a dot in the panel. They do not appear in the human's
  `Mod+Tab` cycle unless the workspace is visited.
- Entering an agent workspace is one binding (`Mod+7/8/9`) and is visible: the
  panel shows which agent owns the space and whether it is waiting for
  approval.
- Killing a session closes its windows and its sandbox, and says so.

Implementation: a `solyra-agent` bridge that owns a workspace mapping and
writes niri window rules by `app_id`; agents launch inside a sandbox whose
`app_id` is set per session.

## Sandbox levels

| Level | Isolation | Status |
| --- | --- | --- |
| 0 | Agent runs as the human, no sandbox | Never offered |
| 1 | Dedicated user `solyra-agent`, `bubblewrap`, policy file, no host home | P5 target |
| 2 | Per-agent micro-VM (QEMU or crosvm), virtio-gpu, PipeWire stream into Agent Space | Later |

Level 1 detail, which is the one that ships first:

- A dedicated user with no login shell, no sudo, and no access to the human's
  home.
- `bubblewrap` with a fresh `/home`, a read-only `/usr`, a per-task writable
  overlay, and the network off unless the policy grants it.
- A policy file per agent — the legacy `cosmos-agent.toml` shape — listing
  which tools are allowed, which need approval, and which are denied.
- An MCP broker in the middle: the agent never talks to a privileged service
  directly, and every call is attributable to a session.

Level 2 is the "own sandbox VM" goal: a VM per agent with its own disk, its
display streamed into Agent Space, and input driven through a virtual seat. It
is not scheduled until level 1 is boring.

## Approvals and audit

- Every sensitive tool call raises an approval card: what will change, which
  paths, which network hosts, and the agent asking.
- Approvals are answered from the shell, with keyboard shortcuts, and expire.
- Every call is appended to a per-agent JSONL trail: timestamp, session, tool,
  arguments hash, decision, result. The trail is the audit surface and the
  input to rollback.
- Denied calls are logged too. Silence is not a policy.

## Undo

Two different problems, two different mechanisms:

- **OS-level changes** → `bootc` rollback. Already the base image's model;
  untested end to end (PLAN.md §4, P1 gate).
- **File changes by an agent** → a snapshot of the files the session touched.

The second one needs a decision before P5: the development disk uses **XFS**,
which has no snapshots. Options are a btrfs root for release images (bootc
supports it, and Fedora's defaults make it the low-friction choice), or a
per-session overlay plus a file-level journal that records prior contents. The
recommendation is btrfs for release images, decided in P5 rather than now,
because the development disk's job is to prove the desktop boots.

## What the first commit does

1. `agent/` crate that starts a level-1 sandbox with `systemd-run` +
   `bubblewrap`, reads a policy file, and prints the resulting identity.
2. A verification that the sandbox cannot see the human's home, has no
   network, and dies with its session.
3. Nothing else. Approvals, Agent Space, and zeron integration come after that
   is boring and tested.

## Non-goals

- Agents are not a chatbot in the panel. There is no "ask the OS" button until
  there is a policy engine behind it.
- No agent runs with the human's credentials by default.
- No silent background actions: anything that changes state is either approved
  or explicitly allowed by policy, and always logged.
