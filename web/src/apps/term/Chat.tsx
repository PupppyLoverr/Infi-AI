import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../../Icon";
import { Chat, EFFORTS, Harness, HARNESS, MODES, Msg, prettyModel } from "./model";

// The chat view of an agent harness (as in the recordings): user turns in
// right-aligned bubbles, the agent's text plain on the left, tool calls as
// dim one-liners, a "Thinking… / Writing… Ns" line, and a rounded composer
// with Auto / model / effort chips and a round send (or stop) button.
export function ChatView({ harness, chat, cwd, onSend, onStop, onChange }: {
  harness: Harness; chat: Chat; cwd: string; onSend: (text: string) => void; onStop: () => void; onChange: (c: Partial<Chat>) => void;
}) {
  const [text, setText] = useState("");
  const [, tick] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const h = HARNESS[harness];
  const busy = chat.phase !== "idle";

  useEffect(() => {
    if (!busy) return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [busy]);
  useLayoutEffect(() => {
    const s = scroller.current;
    if (s) s.scrollTop = s.scrollHeight;
  }, [chat.msgs.length, chat.live]);
  useEffect(() => input.current?.focus(), []);

  const send = () => {
    const t = text.trim();
    if (!t) return;
    onSend(t);
    setText("");
  };
  const cycle = <T,>(list: readonly T[], cur: T) => list[(list.indexOf(cur) + 1) % list.length];
  const secs = Math.max(0, Math.round((Date.now() - (chat.since || Date.now())) / 1000));
  const status = chat.phase === "writing" ? `Writing… ${secs}s` : chat.phase === "tool" ? "Working…" : busy ? "Thinking…" : "";
  const base = cwd.replace(/\/+$/, "").split("/").pop() || "~";

  return (
    <div className="col" style={{ position: "absolute", inset: 0, background: "#161616" }}>
      <div ref={scroller} className="grow" style={{ overflowY: "auto", padding: "26px 0 12px" }}>
        {chat.msgs.length === 0 && !busy ? (
          <div className="col" style={{ height: "100%", alignItems: "center", justifyContent: "center", gap: 10 }}>
            <div style={{ fontSize: 22, fontWeight: 600, color: "#ececec" }}>{h.ask}</div>
            <div style={{ fontSize: 16, color: "#8a8a8a" }}>in {base}</div>
          </div>
        ) : (
          <div className="col" style={{ width: "min(720px, calc(100% - 48px))", margin: "0 auto", gap: 14 }}>
            {chat.msgs.map((m, i) => <Bubble key={i} m={m} />)}
            {chat.live && <div style={{ fontSize: 15, lineHeight: 1.55, color: "#e8e8e8" }}><Markdown text={chat.live} /></div>}
          </div>
        )}
      </div>
      <div style={{ width: "min(736px, calc(100% - 32px))", margin: "0 auto 16px" }}>
        <div style={{ height: 22, padding: "0 8px", fontSize: 14, color: "#8f8f8f" }}>{status}</div>
        <div style={{ borderRadius: 18, background: "#1f1f1f", boxShadow: "inset 0 0 0 1px #2d2d2d", padding: "14px 16px 10px" }}>
          <textarea ref={input} value={text} rows={Math.min(8, Math.max(1, text.split("\n").length))}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
              if (e.key === "Escape" && busy) onStop();
            }}
            placeholder={busy ? h.busy : h.idle}
            style={{ width: "100%", resize: "none", background: "none", border: 0, outline: 0, color: "#ececec", fontSize: 16, lineHeight: 1.45, fontFamily: "inherit" }} />
          <div className="row" style={{ gap: 4, marginTop: 6 }}>
            {harness === "claude" && (
              <>
                <Chip onClick={() => onChange({ mode: cycle(MODES.map((m) => m[0]), chat.mode) })}>{MODES.find((m) => m[0] === chat.mode)?.[1]}</Chip>
                <Chip onClick={() => onChange({ model: cycle(h.models, chat.model as any) as string })}>{chat.model ? prettyModel(chat.model) : chat.reported ? prettyModel(chat.reported) : "Default model"}</Chip>
                <Chip onClick={() => onChange({ effort: cycle(EFFORTS.map((m) => m[0]), chat.effort) })}>{EFFORTS.find((m) => m[0] === chat.effort)?.[1]}</Chip>
              </>
            )}
            {harness === "codex" && <Chip>{chat.reported ? prettyModel(chat.reported) : "Default model"}</Chip>}
            <span className="grow" />
            <button onClick={busy && !text.trim() ? onStop : send} title={busy && !text.trim() ? "Stop" : "Send"}
              style={{ width: 30, height: 30, borderRadius: 15, background: "#f2f2f2", color: "#111", display: "grid", placeItems: "center", opacity: busy || text.trim() ? 1 : 0.45 }}>
              {busy && !text.trim() ? <div style={{ width: 10, height: 10, borderRadius: 2, background: "#111" }} /> : <Icon name="up" size={17} />}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function Chip({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) {
  return (
    <button onClick={onClick} style={{ padding: "4px 8px", borderRadius: 8, fontSize: 14, color: "#a8a8a8" }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "#2a2a2a")} onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}>
      {children}
    </button>
  );
}

function Bubble({ m }: { m: Msg }) {
  switch (m.role) {
    case "user":
      return (
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <div style={{ maxWidth: "75%", padding: "10px 16px", borderRadius: 16, background: "#262626", boxShadow: "inset 0 0 0 1px #2e2e2e", fontSize: 16, color: "#ececec", whiteSpace: "pre-wrap" }}>{m.text}</div>
        </div>
      );
    case "assistant":
      return <div style={{ fontSize: 15, lineHeight: 1.55, color: "#e8e8e8" }}><Markdown text={m.text} /></div>;
    case "thinking":
      return <div style={{ fontSize: 13, color: "#777", fontStyle: "italic" }}>{m.text}</div>;
    case "tool":
      return (
        <div className="row" style={{ gap: 8, fontSize: 13, color: "#9a9a9a", fontFamily: '"JetBrains Mono", monospace' }}>
          <span style={{ width: 7, height: 7, borderRadius: 4, flex: "none", background: m.error ? "#ff6b6b" : "#7ee787" }} />
          <b style={{ color: "#cfcfcf", fontWeight: 600 }}>{m.name}</b>
          <span className="ellipsis">{m.detail}</span>
        </div>
      );
    case "error":
      return <div style={{ fontSize: 13, color: "#ff8f8f", fontFamily: '"JetBrains Mono", monospace', whiteSpace: "pre-wrap" }}>{m.text}</div>;
  }
}

/** A small Markdown subset: fenced code, headings, lists, bold, inline code. */
export function Markdown({ text }: { text: string }) {
  const parts = text.split(/```[^\n]*\n?/);
  return (
    <>
      {parts.map((p, i) =>
        i % 2 ? (
          <pre key={i} style={{ margin: "8px 0", padding: "10px 12px", borderRadius: 10, background: "#0f0f0f", boxShadow: "inset 0 0 0 1px #2a2a2a", overflowX: "auto", fontSize: 13, fontFamily: '"JetBrains Mono", monospace' }}>{p.replace(/\n$/, "")}</pre>
        ) : (
          p.split(/\n{2,}/).filter((b) => b.trim()).map((b, j) => {
            const hm = b.match(/^(#{1,4})\s+(.*)/);
            if (hm) return <div key={`${i}-${j}`} style={{ fontWeight: 700, fontSize: hm[1].length < 3 ? 17 : 15, margin: "10px 0 4px" }}>{inline(hm[2])}</div>;
            if (/^\s*([-*]|\d+\.)\s/.test(b))
              return <ul key={`${i}-${j}`} style={{ margin: "4px 0", paddingLeft: 22 }}>{b.split("\n").filter(Boolean).map((l, k) => <li key={k}>{inline(l.replace(/^\s*([-*]|\d+\.)\s+/, ""))}</li>)}</ul>;
            return <p key={`${i}-${j}`} style={{ margin: "0 0 10px", whiteSpace: "pre-wrap" }}>{inline(b)}</p>;
          })
        ),
      )}
    </>
  );
}

function inline(s: string): React.ReactNode[] {
  return s.split(/(\*\*[^*]+\*\*|`[^`]+`)/).map((t, i) =>
    t.startsWith("**") && t.endsWith("**") ? <b key={i}>{t.slice(2, -2)}</b>
      : t.startsWith("`") && t.endsWith("`") ? <code key={i} style={{ padding: "1px 5px", borderRadius: 5, background: "#262626", fontSize: "0.88em", fontFamily: '"JetBrains Mono", monospace' }}>{t.slice(1, -1)}</code>
      : t,
  );
}
