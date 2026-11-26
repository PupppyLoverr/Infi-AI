import { useCallback, useEffect, useRef, useState } from "react";
import { api, on } from "../api";
import { Icon } from "../Icon";
import { dragProps, TrafficLights } from "./Chrome";
import { ChatView } from "./term/Chat";
import { Chat, fold, Harness, HARNESS, newChat, nextId, Node, Pane, panesOf, removePane, splitAt, Tab } from "./term/model";
import { disposePty, focusPty, XTermPane } from "./term/xterm";

// Solyra Terminal, after the recordings: a tinted glass window with traffic
// lights, a sidebar toggle, tab pills that show one icon per pane plus the
// focused pane's title, split buttons on the right, rounded pane cards, and
// an Agents sidebar. Agent panes switch between a chat view (the harness's
// JSON stream) and the harness's own TUI with the `>_` button; both views
// share one session id.

const GLASS = "rgba(14, 26, 26, 0.58)";

function initial(): { tab: Tab; panes: Pane[]; chat?: Chat; sidebar: boolean } {
  const h = location.hash;
  const agent = h.match(/agent=(claude|codex)/)?.[1] as Harness | undefined;
  const shell = (): Pane => ({ id: nextId(), kind: "shell", view: "tui", pty: nextId(), cwd: null });
  const first: Pane = agent ? { id: nextId(), kind: "agent", harness: agent, view: "chat", pty: null, cwd: null } : shell();
  const panes = [first];
  let root: Node = { pane: first.id };
  if (/layout=split/.test(h)) {
    const b = shell(), c = shell();
    panes.push(b, c);
    root = { dir: "row", a: root, b: { dir: "col", a: { pane: b.id }, b: { pane: c.id }, split: 0.5 }, split: 0.5 };
  }
  return { tab: { id: nextId(), root, focus: first.id }, panes, chat: agent ? newChat() : undefined, sidebar: /sidebar=1/.test(h) };
}

export function Terminal() {
  const [boot] = useState(initial);
  const [tabs, setTabs] = useState<Tab[]>([boot.tab]);
  const [active, setActive] = useState(boot.tab.id);
  const [panes, setPanes] = useState<Record<number, Pane & { title?: string }>>(Object.fromEntries(boot.panes.map((p) => [p.id, p])));
  const chatsRef = useRef<Record<number, Chat & { prompt?: string }>>(boot.chat ? { [boot.panes[0].id]: boot.chat } : {});
  const [, setChatsVer] = useState(0);
  const [sidebar, setSidebar] = useState(boot.sidebar);
  const [menu, setMenu] = useState(false);
  const [harnesses, setHarnesses] = useState<string[]>([]);
  const [home, setHome] = useState("");

  const tab = tabs.find((t) => t.id === active) ?? tabs[0];
  const focused = panes[tab?.focus];
  const chats = chatsRef.current;

  const updateChat = useCallback((id: number, f: (c: Chat & { prompt?: string }) => Chat & { prompt?: string }) => {
    const cur = chatsRef.current[id];
    if (!cur) return;
    chatsRef.current = { ...chatsRef.current, [id]: f(cur) };
    setChatsVer((v) => v + 1);
  }, []);

  useEffect(() => {
    api.harnesses().then(setHarnesses);
    api.homeDir().then(setHome);
  }, []);

  // ---- agent chat turns
  const send = useCallback((id: number, text: string, retry = false) => {
    const pane = panesRef.current[id];
    const c = chatsRef.current[id];
    if (!pane?.harness || !c) return;
    if (!retry) updateChat(id, (c) => ({ ...c, prompt: text, retried: false, msgs: [...c.msgs, { role: "user", text }] }));
    updateChat(id, (c) => ({ ...c, phase: "thinking", since: Date.now(), live: "", last: Date.now() }));
    const cur = chatsRef.current[id];
    api.agentSend(`pane-${id}`, pane.harness, text, { model: cur.model || null, effort: pane.harness === "claude" ? cur.effort : null, mode: pane.harness === "claude" ? cur.mode : null, session: cur.session, fresh: !cur.started, cwd: pane.cwd })
      .catch((e) => updateChat(id, (c) => ({ ...c, phase: "idle", msgs: [...c.msgs, { role: "error", text: String(e?.message ?? e) }] })));
  }, [updateChat]);

  const panesRef = useRef(panes);
  panesRef.current = panes;

  useEffect(() => {
    const a = on<{ key: string; line: string }>("agent-line", ({ key, line }) => {
      const id = Number(key.replace("pane-", ""));
      updateChat(id, (c) => fold(c, line) as Chat & { prompt?: string });
    });
    const b = on<{ key: string; code: number }>("agent-done", ({ key, code }) => {
      const id = Number(key.replace("pane-", ""));
      const c = chatsRef.current[id];
      if (!c) return;
      // A session the TUI never saved: start it fresh, once.
      if (code !== 0 && !c.retried && c.prompt && c.msgs.some((m) => m.role === "error" && /no conversation found/i.test(m.text))) {
        updateChat(id, (c) => ({ ...c, started: false, retried: true, msgs: c.msgs.filter((m) => !(m.role === "error" && /no conversation found/i.test(m.text))) }));
        send(id, c.prompt, true);
        return;
      }
      updateChat(id, (c) => ({ ...c, phase: "idle", live: "", msgs: code !== 0 && c.phase !== "idle" && !c.msgs.some((m) => m.role === "error") ? [...c.msgs, { role: "error", text: `${HARNESS[panesRef.current[id]?.harness ?? "claude"].name} exited (${code})` }] : c.msgs }));
    });
    return () => { a(); b(); };
  }, [send, updateChat]);

  // ---- tabs, panes, splits
  const addTab = (pane: Pane, chat?: Chat) => {
    const t: Tab = { id: nextId(), root: { pane: pane.id }, focus: pane.id };
    if (chat) chatsRef.current = { ...chatsRef.current, [pane.id]: chat };
    setPanes((p) => ({ ...p, [pane.id]: pane }));
    setTabs((ts) => [...ts, t]);
    setActive(t.id);
    setMenu(false);
  };
  const newShell = (cwd: string | null = null): Pane => ({ id: nextId(), kind: "shell", view: "tui", pty: nextId(), cwd });
  const newAgent = (h: Harness, cwd: string | null = null): Pane => ({ id: nextId(), kind: "agent", harness: h, view: "chat", pty: null, cwd });

  const setTab = (id: number, f: (t: Tab) => Tab) => setTabs((ts) => ts.map((t) => (t.id === id ? f(t) : t)));
  const focusPane = (paneId: number) => setTab(tab.id, (t) => ({ ...t, focus: paneId }));

  const split = (dir: "row" | "col") => {
    const p = newShell(focused?.cwd ?? null);
    setPanes((ps) => ({ ...ps, [p.id]: p }));
    setTab(tab.id, (t) => ({ ...t, root: splitAt(t.root, t.focus, dir, p.id), focus: p.id }));
  };

  const closePane = (paneId: number) => {
    const p = panesRef.current[paneId];
    if (p?.pty != null) disposePty(p.pty);
    if (p?.kind === "agent") api.agentStop(`pane-${paneId}`);
    setTabs((ts) => {
      const out: Tab[] = [];
      for (const t of ts) {
        const root = removePane(t.root, paneId);
        if (!root) continue;
        const left = panesOf(root);
        out.push({ ...t, root, focus: left.includes(t.focus) ? t.focus : left[0] });
      }
      if (out.length === 0) api.windowAction("close");
      else if (!out.some((t) => t.id === active)) setActive(out[out.length - 1].id);
      return out;
    });
  };

  const toggleView = (paneId: number) => {
    const p = panesRef.current[paneId];
    const c = chatsRef.current[paneId];
    if (!p || p.kind !== "agent" || !c) return;
    if (p.view === "chat") {
      if (c.phase !== "idle") api.agentStop(`pane-${paneId}`);
      setPanes((ps) => ({ ...ps, [paneId]: { ...p, view: "tui", pty: nextId() } }));
      updateChat(paneId, (c) => ({ ...c, phase: "idle", started: true }));
    } else {
      if (p.pty != null) disposePty(p.pty);
      setPanes((ps) => ({ ...ps, [paneId]: { ...p, view: "chat", pty: null } }));
    }
  };

  const tuiCommand = (p: Pane): string => {
    const c = chatsRef.current[p.id];
    if (p.harness === "codex") return c?.started ? `codex resume ${c.session}` : "codex";
    const parts = ["claude"];
    if (c?.model) parts.push("--model", c.model);
    if (c) parts.push(c.started ? "--resume" : "--session-id", c.session);
    return parts.join(" ");
  };

  // ---- keyboard
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (!(e.ctrlKey && e.shiftKey)) return;
      const act: Record<string, () => void> = {
        KeyT: () => addTab(newShell(focused?.cwd ?? null)),
        KeyD: () => split("row"),
        KeyE: () => split("col"),
        KeyW: () => tab && closePane(tab.focus),
        KeyJ: () => focused && toggleView(focused.id),
        BracketLeft: () => setActive(tabs[(tabs.indexOf(tab) + tabs.length - 1) % tabs.length].id),
        BracketRight: () => setActive(tabs[(tabs.indexOf(tab) + 1) % tabs.length].id),
      };
      if (act[e.code]) { e.preventDefault(); act[e.code](); }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  });

  useEffect(() => {
    if (focused?.pty != null) focusPty(focused.pty);
  }, [active, focused?.id, focused?.pty]);

  // ---- labels
  const short = (cwd: string | null) => (!cwd || cwd === home ? "~" : cwd.startsWith(home) ? "~" + cwd.slice(home.length) : cwd);
  const titleOf = (p: Pane & { title?: string }) => {
    if (p.kind === "agent") {
      const c = chats[p.id];
      const first = c?.msgs.find((m) => m.role === "user") as { text: string } | undefined;
      return p.view === "chat" ? first?.text ?? "New session" : HARNESS[p.harness!].name;
    }
    const t = p.title?.trim();
    if (t) return t.includes(":") ? t.split(":").pop()!.trim() : t;
    return short(p.cwd);
  };

  const chatOnly = !!tab && "pane" in tab.root && focused?.kind === "agent" && focused.view === "chat";
  const cost = focused?.kind === "agent" ? chats[focused.id]?.cost ?? 0 : null;
  const agentPanes = Object.values(panes).filter((p) => p.kind === "agent" && tabs.some((t) => panesOf(t.root).includes(p.id)));

  // ---- render
  const renderNode = (n: Node, path: string): React.ReactNode => {
    if ("pane" in n) {
      const p = panes[n.pane];
      if (!p) return null;
      const isFocus = tab.focus === p.id;
      const bare = chatOnly;
      return (
        <div key={p.id} onMouseDown={() => focusPane(p.id)}
          style={{ position: "relative", flex: 1, minWidth: 0, minHeight: 0, borderRadius: bare ? 0 : 10, overflow: "hidden",
            background: bare ? "transparent" : p.view === "chat" ? "#161616" : "rgba(0,0,0,0.16)",
            boxShadow: bare ? "none" : `inset 0 0 0 1px rgba(255,255,255,${isFocus && panesOf(tab.root).length > 1 ? 0.17 : 0.08})` }}>
          {p.view === "tui" && p.pty != null ? (
            <XTermPane id={p.pty} cmd={p.kind === "agent" ? tuiCommand(p) : null} cwd={p.cwd} focused={isFocus && !menu}
              onFocus={() => focusPane(p.id)} onExit={() => (p.kind === "agent" ? toggleView(p.id) : closePane(p.id))}
              onTitle={(title) => setPanes((ps) => (ps[p.id] && ps[p.id].title !== title ? { ...ps, [p.id]: { ...ps[p.id], title } } : ps))} />
          ) : p.kind === "agent" && chats[p.id] ? (
            <ChatView harness={p.harness!} chat={chats[p.id]} cwd={short(p.cwd)}
              onSend={(t) => send(p.id, t)} onStop={() => api.agentStop(`pane-${p.id}`)}
              onChange={(c) => updateChat(p.id, (x) => ({ ...x, ...c }))} />
          ) : null}
          {!bare && p.kind === "agent" && (
            <button title={p.view === "chat" ? "Open the terminal UI" : "Back to chat"} onClick={() => toggleView(p.id)}
              style={{ position: "absolute", top: 8, right: 10, width: 26, height: 22, borderRadius: 6, fontSize: 12, fontWeight: 700, color: "#bbb", background: "rgba(255,255,255,0.08)", fontFamily: '"JetBrains Mono", monospace' }}>
              {p.view === "chat" ? ">_" : "◐"}
            </button>
          )}
        </div>
      );
    }
    return (
      <Split key={path} n={n} onRatio={(r) => setTab(tab.id, (t) => ({ ...t, root: setRatio(t.root, path, r) }))}>
        {renderNode(n.a, path + "a")}
        {renderNode(n.b, path + "b")}
      </Split>
    );
  };

  return (
    <div className="col" style={{ height: "100%", borderRadius: 14, overflow: "hidden", background: chatOnly ? "#161616" : GLASS, boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.13)", transition: "background 200ms" }}>
      {/* Titlebar */}
      <div className="row" style={{ height: 42, flex: "none", padding: "0 10px 0 16px", gap: 10 }} {...dragProps}>
        <TrafficLights />
        <IconButton title="Agents" onClick={() => setSidebar(!sidebar)} on={sidebar}><SidebarGlyph /></IconButton>
        {chatOnly && tabs.length === 1 ? (
          <div className="row grow" style={{ gap: 8, minWidth: 0 }}>
            <b className="ellipsis" style={{ fontSize: 15, color: "#ececec" }}>{titleOf(focused!)}</b>
            <span style={{ fontSize: 13, color: "#8a8a8a" }}>{short(focused!.cwd).split("/").pop()}</span>
          </div>
        ) : (
          <div className="row grow" style={{ gap: 4, minWidth: 0, overflow: "hidden" }}>
            {tabs.map((t) => {
              const ids = panesOf(t.root);
              const fp = panes[t.focus];
              const on = t.id === tab.id;
              return (
                <button key={t.id} onClick={() => setActive(t.id)} onAuxClick={(e) => e.button === 1 && ids.forEach(closePane)} className="row"
                  style={{ gap: 5, height: 28, padding: "0 9px 0 7px", borderRadius: 8, flex: "none", maxWidth: 260, background: on ? "rgba(255,255,255,0.1)" : "transparent", color: on ? "#eaeaea" : "#a9b4b0" }}>
                  {ids.map((id) => <PaneGlyph key={id} p={panes[id]} />)}
                  {fp && <span className="ellipsis" style={{ fontSize: 13, marginLeft: 3 }}>{titleOf(fp)}</span>}
                </button>
              );
            })}
            <div style={{ position: "relative" }} data-nodrag>
              <IconButton title="New tab" onClick={() => setMenu(!menu)}><Icon name="plus" size={15} /></IconButton>
              {menu && (
                <div className="glass dark" style={{ position: "absolute", top: 32, left: 0, zIndex: 20, minWidth: 210, borderRadius: 12, padding: 5 }}>
                  <MenuItem glyph={<PaneGlyph p={{ kind: "shell" } as Pane} />} label="New Terminal" hint="⌃⇧T" onClick={() => addTab(newShell(focused?.cwd ?? null))} />
                  {(["claude", "codex"] as Harness[]).map((h) => (
                    <MenuItem key={h} glyph={<AgentGlyph h={h} />} label={HARNESS[h].name} hint={harnesses.includes(h) ? "chat" : "not installed"}
                      disabled={!harnesses.includes(h)} onClick={() => addTab(newAgent(h, focused?.cwd ?? null), newChat())} />
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        {cost != null && <span style={{ fontSize: 13, color: "#8a8a8a", fontVariantNumeric: "tabular-nums" }}>${cost.toFixed(2)}</span>}
        {focused?.kind === "agent" && (chatOnly || tabs.length === 1) && (
          <IconButton title={focused.view === "chat" ? "Open the terminal UI (⌃⇧J)" : "Back to chat (⌃⇧J)"} onClick={() => toggleView(focused.id)}>
            <span style={{ fontSize: 12, fontWeight: 700, fontFamily: '"JetBrains Mono", monospace' }}>{focused.view === "chat" ? ">_" : "◐"}</span>
          </IconButton>
        )}
        {!chatOnly && (
          <>
            <IconButton title="Split right (⌃⇧D)" onClick={() => split("row")}><SplitGlyph dir="row" /></IconButton>
            <IconButton title="Split down (⌃⇧E)" onClick={() => split("col")}><SplitGlyph dir="col" /></IconButton>
          </>
        )}
      </div>

      <div className="row grow" style={{ minHeight: 0, alignItems: "stretch" }} onMouseDown={() => menu && setMenu(false)}>
        {sidebar && (
          <div className="col" style={{ width: 236, flex: "none", padding: "6px 10px", gap: 4 }}>
            <div style={{ fontSize: 13, fontWeight: 600, color: "#c9d3cf", padding: "4px 6px 8px" }}>Agents <span style={{ color: "#7f8a86", fontWeight: 500, marginLeft: 4 }}>{agentPanes.length}</span></div>
            {agentPanes.map((p) => {
              const c = chats[p.id];
              const t = tabs.find((t) => panesOf(t.root).includes(p.id))!;
              const state = !c ? "Idle" : c.phase === "idle" ? "Idle" : c.phase === "writing" ? "Writing" : c.phase === "tool" ? "Working" : "Thinking";
              return (
                <button key={p.id} onClick={() => { setActive(t.id); setTab(t.id, (x) => ({ ...x, focus: p.id })); }} className="row"
                  style={{ gap: 10, padding: "8px 8px", borderRadius: 10, alignItems: "flex-start", textAlign: "left", background: tab.focus === p.id ? "rgba(255,255,255,0.08)" : "transparent" }}>
                  <div style={{ paddingTop: 2 }}><AgentGlyph h={p.harness!} size={18} /></div>
                  <div className="col grow" style={{ gap: 3, minWidth: 0 }}>
                    <div className="row"><b className="grow ellipsis" style={{ fontSize: 14, fontWeight: 550, color: "#eef2f0" }}>{HARNESS[p.harness!].name}</b><span style={{ fontSize: 11, color: "#7f8a86" }}>{ago(c?.last)}</span></div>
                    <div className="row" style={{ gap: 6, fontSize: 12, color: "#8d9894" }}>
                      <span style={{ width: 7, height: 7, borderRadius: 4, boxShadow: state === "Idle" ? "inset 0 0 0 1.3px #8d9894" : "none", background: state === "Idle" ? "transparent" : "#7ee787" }} />
                      <span className="ellipsis">{state} · {short(p.cwd)}</span>
                    </div>
                  </div>
                </button>
              );
            })}
            {agentPanes.length === 0 && <div style={{ fontSize: 12, color: "#7f8a86", padding: "0 6px" }}>Start Claude Code or Codex from the + menu.</div>}
          </div>
        )}
        <div className="row grow" style={{ minWidth: 0, padding: chatOnly ? 0 : "0 8px 8px", alignItems: "stretch" }}>
          {tab && renderNode(tab.root, "")}
        </div>
      </div>
    </div>
  );
}

function setRatio(n: Node, path: string, r: number): Node {
  if ("pane" in n) return n;
  if (path === "") return { ...n, split: r };
  return path[0] === "a" ? { ...n, a: setRatio(n.a, path.slice(1), r) } : { ...n, b: setRatio(n.b, path.slice(1), r) };
}

function Split({ n, children, onRatio }: { n: Extract<Node, { dir: string }>; children: React.ReactNode[]; onRatio: (r: number) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const row = n.dir === "row";
  const drag = (e: React.MouseEvent) => {
    e.preventDefault();
    const r0 = box.current!.getBoundingClientRect();
    const move = (m: MouseEvent) => onRatio(Math.min(0.85, Math.max(0.15, row ? (m.clientX - r0.left) / r0.width : (m.clientY - r0.top) / r0.height)));
    const up = () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return (
    <div ref={box} style={{ display: "flex", flexDirection: row ? "row" : "column", flex: 1, minWidth: 0, minHeight: 0 }}>
      <div style={{ display: "flex", flex: `${n.split} 1 0`, minWidth: 0, minHeight: 0 }}>{children[0]}</div>
      <div onMouseDown={drag} style={{ flex: "none", [row ? "width" : "height"]: 6, cursor: row ? "col-resize" : "row-resize" } as any} />
      <div style={{ display: "flex", flex: `${1 - n.split} 1 0`, minWidth: 0, minHeight: 0 }}>{children[1]}</div>
    </div>
  );
}

function ago(t?: number) {
  if (!t) return "now";
  const s = (Date.now() - t) / 1000;
  return s < 60 ? "now" : s < 3600 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h`;
}

function IconButton({ children, onClick, title, on }: { children: React.ReactNode; onClick: () => void; title: string; on?: boolean }) {
  return (
    <button title={title} onClick={onClick}
      style={{ width: 28, height: 26, borderRadius: 7, display: "grid", placeItems: "center", color: "#b6c1bd", background: on ? "rgba(255,255,255,0.1)" : "transparent", flex: "none" }}
      onMouseEnter={(e) => !on && (e.currentTarget.style.background = "rgba(255,255,255,0.07)")} onMouseLeave={(e) => !on && (e.currentTarget.style.background = "transparent")}>
      {children}
    </button>
  );
}

function MenuItem({ glyph, label, hint, onClick, disabled }: { glyph: React.ReactNode; label: string; hint?: string; onClick: () => void; disabled?: boolean }) {
  return (
    <button onClick={disabled ? undefined : onClick} className="row" style={{ width: "100%", gap: 9, padding: "7px 9px", borderRadius: 8, fontSize: 13, opacity: disabled ? 0.45 : 1 }}
      onMouseEnter={(e) => !disabled && (e.currentTarget.style.background = "rgba(255,255,255,0.1)")} onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}>
      {glyph}<span className="grow" style={{ textAlign: "left" }}>{label}</span><span style={{ fontSize: 11, color: "#8a8a8a" }}>{hint}</span>
    </button>
  );
}

function PaneGlyph({ p }: { p?: Pane }) {
  if (p?.kind === "agent") return <AgentGlyph h={p.harness!} size={14} />;
  return (
    <span style={{ width: 16, height: 14, borderRadius: 4, display: "grid", placeItems: "center", boxShadow: "inset 0 0 0 1.2px currentColor", opacity: 0.75, fontSize: 7, fontWeight: 800, fontFamily: '"JetBrains Mono", monospace' }}>&gt;_</span>
  );
}

/** Harness marks: an orange spark for Claude Code, a knot for Codex (drawn here, not the vendors' logos). */
export function AgentGlyph({ h, size = 16 }: { h: Harness; size?: number }) {
  if (h === "claude")
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" style={{ flex: "none" }}>
        <g stroke="#d97757" strokeWidth="2.6" strokeLinecap="round">
          {[0, 45, 90, 135, 180, 225, 270, 315].map((a) => <line key={a} x1="12" y1="12" x2="12" y2={a % 90 ? 4.5 : 2.5} transform={`rotate(${a} 12 12)`} />)}
        </g>
      </svg>
    );
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" style={{ flex: "none" }} fill="none" stroke="#e6e6e6" strokeWidth="1.8">
      <circle cx="12" cy="12" r="9" /><path d="M8 9.5l3 2.5-3 2.5M12.5 15h3.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function SidebarGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="3" y="4.5" width="18" height="15" rx="3.5" /><path d="M9 5v14" /><path d="M5.5 8.5h1.5M5.5 11.5h1.5" strokeLinecap="round" />
    </svg>
  );
}

function SplitGlyph({ dir }: { dir: "row" | "col" }) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="3" y="4.5" width="18" height="15" rx="3.5" />{dir === "row" ? <path d="M12 5v14" /> : <path d="M3.5 12h17" />}
    </svg>
  );
}
