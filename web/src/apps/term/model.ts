// Tabs hold a split tree of panes. A pane is a shell, or an agent harness
// shown either as the chat view (headless JSON stream) or its own TUI.

export type Harness = "claude" | "codex";
export type Pane = {
  id: number;
  kind: "shell" | "agent";
  harness?: Harness;
  view: "chat" | "tui";
  pty: number | null; // PTY behind the shell or the agent TUI
  cwd: string | null;
};
export type Node = { pane: number } | { dir: "row" | "col"; a: Node; b: Node; split: number };
export type Tab = { id: number; root: Node; focus: number };

export type Msg =
  | { role: "user"; text: string }
  | { role: "assistant"; text: string }
  | { role: "thinking"; text: string }
  | { role: "tool"; name: string; detail: string; error?: boolean }
  | { role: "error"; text: string };

export type Chat = {
  msgs: Msg[];
  live: string;
  phase: "idle" | "thinking" | "writing" | "tool";
  since: number;
  cost: number;
  session: string;
  started: boolean; // the harness has a record of `session`
  model: string; // alias the user picked ("" = harness default)
  effort: string;
  mode: string;
  reported: string; // model name the harness reported
  last: number; // last activity, for the sidebar
  retried?: boolean;
};

let seq = 1;
export const nextId = () => seq++;

export const HARNESS = {
  claude: { name: "Claude Code", models: ["", "opus", "sonnet", "haiku"], ask: "What should Claude do?", busy: "Add to what Claude is doing", idle: "Ask Claude to do something" },
  codex: { name: "Codex", models: [""], ask: "What should Codex do?", busy: "Codex is working…", idle: "Ask Codex to do something" },
} as const;

export const MODES: [string, string][] = [["auto", "Auto"], ["acceptEdits", "Accept edits"], ["plan", "Plan"], ["manual", "Ask"]];
export const EFFORTS: [string, string][] = [["low", "Low"], ["medium", "Medium"], ["high", "High"], ["max", "Max"]];

export function newChat(): Chat {
  return { msgs: [], live: "", phase: "idle", since: 0, cost: 0, session: crypto.randomUUID(), started: false, model: "", effort: "medium", mode: "auto", reported: "", last: Date.now() };
}

/** "claude-opus-5-5" → "Opus 5.5"; aliases → capitalised. */
export function prettyModel(m: string): string {
  const x = m.match(/(opus|sonnet|haiku|fable)-(\d+)(?:-(\d+))?/i);
  if (x) return `${x[1][0].toUpperCase()}${x[1].slice(1)} ${x[2]}${x[3] && x[3].length < 3 ? "." + x[3] : ""}`;
  return m ? m[0].toUpperCase() + m.slice(1) : "Default";
}

export function panesOf(n: Node): number[] {
  return "pane" in n ? [n.pane] : [...panesOf(n.a), ...panesOf(n.b)];
}

export function splitAt(n: Node, target: number, dir: "row" | "col", fresh: number): Node {
  if ("pane" in n) return n.pane === target ? { dir, a: n, b: { pane: fresh }, split: 0.5 } : n;
  return { ...n, a: splitAt(n.a, target, dir, fresh), b: splitAt(n.b, target, dir, fresh) };
}

export function removePane(n: Node, target: number): Node | null {
  if ("pane" in n) return n.pane === target ? null : n;
  const a = removePane(n.a, target);
  const b = removePane(n.b, target);
  if (!a) return b;
  if (!b) return a;
  return { ...n, a, b };
}

/** One step of a harness JSON stream folded into the chat. */
export function fold(c: Chat, line: string): Chat {
  let ev: any;
  try { ev = JSON.parse(line); } catch { return c; }
  const now = Date.now();
  const add = (m: Msg) => ({ ...c, msgs: [...c.msgs, m], last: now });
  switch (ev.type) {
    // ---- Claude Code (stream-json)
    case "system":
      if (ev.subtype === "init") return { ...c, reported: ev.model ?? c.reported, started: true, session: ev.session_id ?? c.session };
      return c;
    case "stream_event": {
      const e = ev.event ?? {};
      if (e.type === "content_block_delta" && e.delta?.type === "text_delta")
        return { ...c, live: c.live + e.delta.text, phase: "writing", since: c.phase === "writing" ? c.since : now, last: now };
      if (e.type === "content_block_start" && e.content_block?.type === "thinking") return { ...c, phase: "thinking" };
      return c;
    }
    case "assistant": {
      let out = { ...c, live: "", last: now };
      for (const b of ev.message?.content ?? []) {
        if (b.type === "text" && b.text?.trim()) out = { ...out, msgs: [...out.msgs, { role: "assistant", text: b.text }] };
        else if (b.type === "tool_use") out = { ...out, phase: "tool", msgs: [...out.msgs, { role: "tool", name: b.name, detail: toolDetail(b.input) }] };
      }
      return out;
    }
    case "user": {
      const errs = (ev.message?.content ?? []).filter((b: any) => b.type === "tool_result" && b.is_error);
      if (!errs.length) return { ...c, phase: "thinking" };
      const msgs = [...c.msgs];
      for (let i = msgs.length - 1; i >= 0; i--) if (msgs[i].role === "tool") { msgs[i] = { ...(msgs[i] as any), error: true }; break; }
      return { ...c, msgs, phase: "thinking" };
    }
    case "result": {
      const cost = typeof ev.total_cost_usd === "number" ? ev.total_cost_usd : 0;
      const base = { ...c, live: "", phase: "idle" as const, cost: c.cost + cost, session: ev.session_id ?? c.session, started: true, last: now };
      if (ev.is_error && ev.result) return { ...base, msgs: [...base.msgs, { role: "error", text: String(ev.result) }] };
      return base;
    }
    case "stderr":
      return ev.text?.trim() ? add({ role: "error", text: ev.text }) : c;
    // ---- Codex (exec --json)
    case "thread.started":
      return { ...c, session: ev.thread_id ?? c.session, started: true };
    case "item.started":
      if (ev.item?.type === "command_execution") return { ...c, phase: "tool" };
      return c;
    case "item.completed": {
      const it = ev.item ?? {};
      if (it.type === "agent_message") return { ...add({ role: "assistant", text: it.text ?? "" }), phase: "writing", since: now };
      if (it.type === "reasoning") return { ...c, phase: "thinking" };
      if (it.type === "command_execution") return add({ role: "tool", name: "Bash", detail: it.command ?? "", error: it.exit_code ? it.exit_code !== 0 : false });
      if (it.type === "file_change") return add({ role: "tool", name: "Edit", detail: (it.changes ?? []).map((x: any) => x.path).join(", ") });
      return c;
    }
    case "turn.completed":
      return { ...c, phase: "idle", live: "" };
    case "error":
      return add({ role: "error", text: ev.message ?? "error" });
  }
  return c;
}

function toolDetail(input: any): string {
  if (!input || typeof input !== "object") return "";
  const v = input.command ?? input.file_path ?? input.path ?? input.pattern ?? input.url ?? input.description ?? input.prompt;
  return typeof v === "string" ? v : JSON.stringify(input).slice(0, 120);
}
