import { useEffect, useMemo, useRef, useState } from "react";
import { api, App, Clip, Entry, on, surfaceShape } from "../api";
import { AppIcon, Icon } from "../Icon";

// macOS 27 Spotlight: a floating glass capsule with the query on the left
// and category pills on the right; results drop into a glass sheet below,
// grouped (Top Hit, Applications, Files, Clipboard, Actions). Tab cycles
// the category, arrows move, Enter opens, Esc closes.
type Filter = "All" | "Apps" | "Files" | "Clipboard" | "Actions";
const FILTERS: Filter[] = ["All", "Apps", "Files", "Clipboard", "Actions"];

type Result = { group: string; title: string; sub: string; icon: { role?: string; glyph?: string }; run: () => void };

const ACTIONS: { title: string; sub: string; glyph: string; cmd: () => void }[] = [
  { title: "Lock Screen", sub: "Lock this session", glyph: "lock", cmd: () => api.launch("solyra-panel lock") },
  { title: "Control Centre", sub: "Wi-Fi, sound, display", glyph: "settings", cmd: () => api.surfaceToggle("control") },
  { title: "New Terminal", sub: "Open a terminal window", glyph: "terminal", cmd: () => api.launch("solyra-web terminal") },
  { title: "Home Folder", sub: "Open Files at ~", glyph: "home", cmd: () => api.launch("solyra-web files") },
  { title: "Downloads", sub: "Open Files at ~/Downloads", glyph: "download", cmd: () => api.launch('solyra-web files "$HOME/Downloads"') },
  { title: "Overview", sub: "Show all windows", glyph: "grid", cmd: () => api.launch("niri msg action toggle-overview") },
];

/** Tiny, safe calculator: digits, operators, brackets, decimal points. */
export function calc(q: string): string | null {
  const s = q.replace(/\s+/g, "").replace(/×/g, "*").replace(/÷/g, "/");
  if (!/^[\d.+\-*/()%^]+$/.test(s) || !/\d/.test(s) || !/[+\-*/%^]/.test(s)) return null;
  try {
    // eslint-disable-next-line no-new-func
    const v = Function(`"use strict";return (${s.replace(/\^/g, "**")})`)();
    if (typeof v !== "number" || !isFinite(v)) return null;
    return String(Math.round(v * 1e10) / 1e10);
  } catch {
    return null;
  }
}

const score = (q: string, s: string) => {
  const a = s.toLowerCase();
  if (a.startsWith(q)) return 3;
  if (a.split(/[\s\-_.]/).some((w) => w.startsWith(q))) return 2;
  return a.includes(q) ? 1 : 0;
};

export function Spotlight() {
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("All");
  const [sel, setSel] = useState(0);
  const [apps, setApps] = useState<App[]>([]);
  const [files, setFiles] = useState<Entry[]>([]);
  const [clips, setClips] = useState<Clip[]>([]);
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);

  const refresh = () => {
    api.apps().then(setApps);
    api.clipboard().then(setClips);
  };
  useEffect(() => {
    refresh();
    input.current?.focus();
    const offQuery = on<string>("spotlight-query", (text) => { setQ(text); window.setTimeout(() => input.current?.focus(), 30); });
    const offShown = on<{ shown: boolean }>("surface-shown", (e) => {
      if (e.shown) {
        setQ("");
        setFilter("All");
        setSel(0);
        refresh();
        window.setTimeout(() => input.current?.focus(), 30);
      }
    });
    return () => { offQuery(); offShown(); };
  }, []);

  useEffect(() => {
    const t = q.trim().length >= 2 && (filter === "All" || filter === "Files") ? window.setTimeout(() => api.findFiles(q.trim()).then(setFiles), 140) : 0;
    if (!t) setFiles([]);
    return () => window.clearTimeout(t);
  }, [q, filter]);

  const results: Result[] = useMemo(() => {
    const ql = q.trim().toLowerCase();
    const out: Result[] = [];
    const want = (f: Filter) => filter === "All" || filter === f;
    const close = () => api.surfaceHide();
    const c = calc(q);
    if (c != null && filter === "All")
      out.push({ group: "Calculator", title: c, sub: `${q.trim()} =  · Enter to copy`, icon: { role: "calculator" }, run: () => { api.copyText(c); close(); } });
    if (want("Apps")) {
      const list = ql ? apps.map((a) => [Math.max(score(ql, a.name) * 2, score(ql, a.comment)), a] as const).filter(([s]) => s > 0).sort((x, y) => y[0] - x[0]).map(([, a]) => a) : apps;
      list.slice(0, ql ? (filter === "Apps" ? 12 : 6) : filter === "Apps" ? 24 : 8).forEach((a) =>
        out.push({ group: "Applications", title: a.name, sub: a.comment || "Application", icon: { role: a.role || "overview" }, run: () => { api.launch(a.exec); close(); } }));
    }
    if (want("Files"))
      files.slice(0, filter === "Files" ? 14 : 5).forEach((f) =>
        out.push({ group: "Files", title: f.name, sub: f.path, icon: { role: f.is_dir ? "folder" : undefined, glyph: f.is_dir ? undefined : "document" }, run: () => { api.openPath(f.path); close(); } }));
    if (want("Clipboard")) {
      const list = ql ? clips.filter((c) => c.text.toLowerCase().includes(ql)) : clips;
      list.slice(0, filter === "Clipboard" ? 12 : ql ? 3 : 0).forEach((c) =>
        out.push({ group: "Clipboard", title: c.text.replace(/\s+/g, " ").slice(0, 80), sub: `${c.kind} · Enter to copy`, icon: { glyph: c.kind === "link" ? "link" : c.kind === "color" ? "palette" : "clipboard" }, run: () => { api.copyText(c.text); close(); } }));
    }
    if (want("Actions"))
      ACTIONS.filter((a) => !ql || score(ql, a.title) > 0).slice(0, filter === "Actions" ? 12 : ql ? 3 : 0).forEach((a) =>
        out.push({ group: "Actions", title: a.title, sub: a.sub, icon: { glyph: a.glyph }, run: () => { a.cmd(); close(); } }));
    if (out.length && ql && out[0].group === "Applications") out[0] = { ...out[0], group: "Top Hit" };
    return out;
  }, [q, filter, apps, files, clips]);

  useEffect(() => setSel(0), [q, filter]);
  const cap = useRef<HTMLDivElement>(null);
  const sheet = useRef<HTMLDivElement>(null);
  useEffect(() => surfaceShape([box.current], [[cap.current, 28], [sheet.current, 24]], "regular"));

  const key = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") api.surfaceHide();
    else if (e.key === "Tab") {
      e.preventDefault();
      const i = FILTERS.indexOf(filter);
      setFilter(FILTERS[(i + (e.shiftKey ? FILTERS.length - 1 : 1)) % FILTERS.length]);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(results.length - 1, s + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(0, s - 1));
    } else if (e.key === "Enter") results[sel]?.run();
  };

  let last = "";
  return (
    <div style={{ height: "100%", display: "flex", justifyContent: "center", paddingTop: 120 }}>
      <div ref={box} className="col" style={{ width: 680, gap: 10, alignSelf: "flex-start" }}>
        <div ref={cap} className="glass row" style={{ height: 56, borderRadius: 28, padding: "0 10px 0 20px", gap: 12 }}>
          <Icon name="search" size={22} style={{ opacity: 0.7 }} />
          <input ref={input} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={key} placeholder="Spotlight Search" spellCheck={false}
            className="grow" style={{ fontSize: 22, fontWeight: 400, background: "none", border: 0, outline: 0, color: "var(--ink)" }} />
          <div className="row" style={{ gap: 4 }}>
            {FILTERS.slice(1).map((f) => (
              <button key={f} onClick={() => { setFilter(filter === f ? "All" : f); input.current?.focus(); }} className="pill"
                style={{ padding: "5px 11px", fontSize: 12, fontWeight: 550, background: filter === f ? "var(--accent)" : "rgba(255,255,255,0.1)", color: filter === f ? "#fff" : "var(--ink-2)" }}>
                {f}
              </button>
            ))}
          </div>
        </div>
        {results.length > 0 && (
          <div ref={sheet} className="glass" style={{ borderRadius: 24, padding: 8, maxHeight: 390, overflowY: "auto" }}>
            {results.map((r, i) => {
              const head = r.group !== last ? r.group : null;
              last = r.group;
              return (
                <div key={i}>
                  {head && <div style={{ padding: "8px 12px 4px", fontSize: 11, fontWeight: 650, color: "var(--ink-3)", letterSpacing: 0.2 }}>{head}</div>}
                  <button onMouseEnter={() => setSel(i)} onClick={r.run} className="row"
                    style={{ width: "100%", gap: 12, padding: "7px 12px", borderRadius: 12, textAlign: "left", background: i === sel ? "var(--accent)" : "transparent", color: i === sel ? "#fff" : "var(--ink)" }}>
                    {r.icon.role ? <AppIcon role={r.icon.role} size={head === "Top Hit" ? 40 : 30} /> : (
                      <div style={{ width: 30, height: 30, borderRadius: 8, display: "grid", placeItems: "center", background: i === sel ? "rgba(255,255,255,0.2)" : "rgba(255,255,255,0.1)" }}>
                        <Icon name={r.icon.glyph ?? "info"} size={17} />
                      </div>
                    )}
                    <div className="col grow">
                      <span className="ellipsis" style={{ fontSize: head === "Top Hit" ? 16 : 14, fontWeight: 550 }}>{r.title}</span>
                      <span className="ellipsis" style={{ fontSize: 12, opacity: 0.65 }}>{r.sub}</span>
                    </div>
                    {i === sel && <span style={{ fontSize: 11, opacity: 0.8 }}>↵</span>}
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
