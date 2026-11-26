import { useEffect, useMemo, useState } from "react";
import { api, Entry } from "../api";
import { AppIcon, Icon } from "../Icon";
import { dragProps, TrafficLights } from "./Chrome";

// macOS 27 Finder: an inset glass sidebar floating over the window,
// a toolbar of capsule groups (history, title, view switcher, actions,
// search), icon / list / column views, and a path bar.
type View = "icons" | "list" | "columns";

const TAGS: [string, string][] = [["Red", "#ff453a"], ["Orange", "#ff9f0a"], ["Yellow", "#ffd60a"], ["Green", "#32d74b"], ["Blue", "#0a84ff"], ["Purple", "#bf5af2"]];

function initialPath(): string | null {
  const m = location.hash.match(/path=([^&]+)/);
  return m ? decodeURIComponent(m[1]) : null;
}

const join = (a: string, b: string) => (a.endsWith("/") ? a + b : `${a}/${b}`);
const base = (p: string) => p.replace(/\/+$/, "").split("/").pop() || "/";
const parent = (p: string) => p.replace(/\/+$/, "").replace(/\/[^/]*$/, "") || "/";

function fmtSize(n: number) {
  if (n < 1024) return `${n} bytes`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024, i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${u[i]}`;
}
const fmtDate = (s: number) => (s ? new Date(s * 1000).toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit" }) : "--");

function glyphFor(kind: string) {
  if (kind.startsWith("Image")) return { g: "image", c: "#30b0c7" };
  if (kind.startsWith("Video")) return { g: "play", c: "#5e5ce6" };
  if (kind.startsWith("Audio")) return { g: "music", c: "#ff375f" };
  if (kind.startsWith("Source")) return { g: "terminal", c: "#8e8e93" };
  if (kind.startsWith("Archive")) return { g: "tray", c: "#a2845e" };
  if (kind.startsWith("PDF")) return { g: "document", c: "#ff453a" };
  return { g: "document", c: "#64a0ff" };
}

function FileIcon({ e, size }: { e: Entry; size: number }) {
  if (e.is_dir) return <AppIcon role={/Downloads$/.test(e.path) ? "downloads" : "folder"} size={size} />;
  const { g, c } = glyphFor(e.kind);
  const ext = e.name.includes(".") ? e.name.split(".").pop()!.slice(0, 4).toUpperCase() : "";
  return (
    <div style={{ width: size * 0.78, height: size, margin: `0 ${size * 0.11}px`, borderRadius: size * 0.08, position: "relative", background: "linear-gradient(#fdfdfd,#e9e9ee)", boxShadow: "0 1px 3px rgba(0,0,0,0.35)", display: "grid", placeItems: "center" }}>
      <Icon name={g} size={size * 0.38} style={{ color: c }} />
      {size >= 40 && ext && <span style={{ position: "absolute", bottom: size * 0.08, fontSize: size * 0.13, fontWeight: 700, color: c }}>{ext}</span>}
    </div>
  );
}

export function Files() {
  const [home, setHome] = useState("/home");
  const [path, setPath] = useState<string>("");
  const [back, setBack] = useState<string[]>([]);
  const [fwd, setFwd] = useState<string[]>([]);
  const [entries, setEntries] = useState<Entry[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [view, setView] = useState<View>("icons");
  const [sel, setSel] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [hidden, setHidden] = useState(false);
  const [cols, setCols] = useState<Record<string, Entry[]>>({});

  useEffect(() => {
    api.homeDir().then((h) => {
      setHome(h);
      setPath(initialPath()?.replace(/^~|^\$HOME/, h) ?? h);
    });
  }, []);

  useEffect(() => {
    if (!path) return;
    setErr(null);
    setSel(null);
    api.listDir(path, hidden).then(setEntries).catch((e) => setErr(String(e)));
  }, [path, hidden]);

  // Column view: list every ancestor from home (or /) down to `path`.
  const chain = useMemo(() => {
    if (!path) return [];
    const root = path.startsWith(home) ? home : "/";
    const rest = path.slice(root.length).split("/").filter(Boolean);
    const out = [root];
    rest.forEach((p) => out.push(join(out[out.length - 1], p)));
    return out;
  }, [path, home]);
  useEffect(() => {
    if (view !== "columns") return;
    chain.forEach((d) => api.listDir(d, hidden).then((l) => setCols((c) => ({ ...c, [d]: l }))));
  }, [chain, view, hidden]);

  const go = (p: string) => {
    if (p === path) return;
    setBack((b) => [...b, path]);
    setFwd([]);
    setPath(p);
    setQ("");
  };
  const open = (e: Entry) => (e.is_dir ? go(e.path) : api.openPath(e.path));
  const shown = q ? entries.filter((e) => e.name.toLowerCase().includes(q.toLowerCase())) : entries;
  const selected = entries.find((e) => e.path === sel) ?? null;

  const side: { label: string; items: { name: string; path: string; glyph: string }[] }[] = [
    { label: "Favourites", items: [
      { name: "Home", path: home, glyph: "home" },
      { name: "Desktop", path: join(home, "Desktop"), glyph: "desktop" },
      { name: "Documents", path: join(home, "Documents"), glyph: "document" },
      { name: "Downloads", path: join(home, "Downloads"), glyph: "download" },
      { name: "Pictures", path: join(home, "Pictures"), glyph: "image" },
      { name: "Music", path: join(home, "Music"), glyph: "music" },
    ] },
    { label: "Locations", items: [
      { name: "Computer", path: "/", glyph: "desktop" },
      { name: "Trash", path: join(home, ".local/share/Trash/files"), glyph: "trash" },
    ] },
  ];

  return (
    <div style={{ height: "100%", padding: 0 }}>
      <div className="row" style={{ height: "100%", borderRadius: 16, overflow: "hidden", background: "rgba(28,28,32,0.72)", boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.14)" }}>
        {/* Inset glass sidebar */}
        <div className="col glass" style={{ width: 200, margin: 8, marginRight: 0, height: "calc(100% - 16px)", borderRadius: 14, padding: "12px 8px", gap: 2, ["--glass-tint" as any]: "rgba(60,60,70,0.35)" }} {...dragProps}>
          <div style={{ padding: "0 6px 14px" }}><TrafficLights /></div>
          {side.map((g) => (
            <div key={g.label} className="col" style={{ gap: 1, marginBottom: 12 }}>
              <div style={{ fontSize: 11, fontWeight: 650, color: "var(--ink-3)", padding: "0 8px 4px" }}>{g.label}</div>
              {g.items.map((it) => (
                <button key={it.path} onClick={() => go(it.path)} className="row"
                  style={{ gap: 8, padding: "5px 8px", borderRadius: 8, fontSize: 13, background: path === it.path ? "rgba(255,255,255,0.14)" : "transparent" }}>
                  <Icon name={it.glyph} size={16} style={{ color: "var(--accent)" }} />
                  <span className="ellipsis">{it.name}</span>
                </button>
              ))}
            </div>
          ))}
          <div className="col" style={{ gap: 1 }}>
            <div style={{ fontSize: 11, fontWeight: 650, color: "var(--ink-3)", padding: "0 8px 4px" }}>Tags</div>
            {TAGS.map(([n, c]) => (
              <div key={n} className="row" style={{ gap: 8, padding: "4px 8px", fontSize: 13 }}>
                <span style={{ width: 10, height: 10, borderRadius: 5, background: c }} />{n}
              </div>
            ))}
          </div>
        </div>

        <div className="col grow" style={{ height: "100%" }}>
          {/* Toolbar */}
          <div className="row" style={{ height: 52, padding: "0 12px 0 14px", gap: 10 }} {...dragProps}>
            <div className="row glass clear pill" style={{ padding: 3 }}>
              <button disabled={!back.length} onClick={() => { const p = back[back.length - 1]; setBack(back.slice(0, -1)); setFwd([path, ...fwd]); setPath(p); }}
                style={{ width: 30, height: 26, opacity: back.length ? 1 : 0.35 }}><Icon name="left" size={16} /></button>
              <button disabled={!fwd.length} onClick={() => { const [p, ...r] = fwd; setFwd(r); setBack([...back, path]); setPath(p); }}
                style={{ width: 30, height: 26, opacity: fwd.length ? 1 : 0.35 }}><Icon name="right" size={16} /></button>
            </div>
            <b className="ellipsis grow" style={{ fontSize: 15 }}>{path === home ? "Home" : base(path)}</b>
            <div className="row glass clear pill" style={{ padding: 3 }} data-nodrag>
              {(["icons", "list", "columns"] as View[]).map((v) => (
                <button key={v} onClick={() => setView(v)} className="pill" title={v}
                  style={{ width: 32, height: 26, display: "grid", placeItems: "center", background: view === v ? "rgba(255,255,255,0.18)" : "transparent" }}>
                  <Icon name={v === "icons" ? "grid" : v} size={15} />
                </button>
              ))}
            </div>
            <div className="row glass clear pill" style={{ padding: 3 }}>
              <button title="Add to Notch Shelf" disabled={!selected} onClick={() => selected && api.shelfAdd(selected.path)} style={{ width: 32, height: 26, opacity: selected ? 1 : 0.4 }}><Icon name="tray" size={15} /></button>
              <button title="Copy path" onClick={() => api.copyText(selected?.path ?? path)} style={{ width: 32, height: 26 }}><Icon name="share" size={15} /></button>
              <button title={hidden ? "Hide hidden files" : "Show hidden files"} onClick={() => setHidden(!hidden)} style={{ width: 32, height: 26, opacity: hidden ? 1 : 0.6 }}><Icon name="eye" size={15} /></button>
            </div>
            <div className="row glass clear pill" style={{ height: 32, width: 180, padding: "0 10px", gap: 6 }}>
              <Icon name="search" size={14} style={{ opacity: 0.6 }} />
              <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search" className="grow" style={{ background: "none", border: 0, outline: 0, color: "var(--ink)", fontSize: 13 }} />
            </div>
          </div>

          {/* Content */}
          <div className="grow" style={{ overflow: "auto", padding: view === "columns" ? 0 : "6px 14px" }} onClick={(e) => e.target === e.currentTarget && setSel(null)}>
            {err ? (
              <div className="col" style={{ height: "100%", alignItems: "center", justifyContent: "center", color: "var(--ink-3)", gap: 8 }}>
                <Icon name="lock" size={28} />{err}
              </div>
            ) : view === "icons" ? (
              <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(104px, 1fr))", gap: 6 }}>
                {shown.map((e) => (
                  <button key={e.path} onClick={() => setSel(e.path)} onDoubleClick={() => open(e)} className="col" title={e.name}
                    style={{ alignItems: "center", gap: 6, padding: "10px 4px" }}>
                    <div style={{ padding: 4, borderRadius: 8, background: sel === e.path ? "rgba(255,255,255,0.12)" : "transparent" }}><FileIcon e={e} size={60} /></div>
                    <span style={{ fontSize: 12, maxWidth: 98, padding: "1px 5px", borderRadius: 4, textAlign: "center", wordBreak: "break-word", lineHeight: 1.25, background: sel === e.path ? "var(--accent)" : "transparent", display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>{e.name}</span>
                  </button>
                ))}
              </div>
            ) : view === "list" ? (
              <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
                <thead>
                  <tr style={{ color: "var(--ink-3)", fontSize: 11, textAlign: "left" }}>
                    <th style={{ padding: "4px 8px", fontWeight: 600 }}>Name</th><th style={{ fontWeight: 600 }}>Date Modified</th><th style={{ fontWeight: 600 }}>Size</th><th style={{ fontWeight: 600 }}>Kind</th>
                  </tr>
                </thead>
                <tbody>
                  {shown.map((e, i) => (
                    <tr key={e.path} onClick={() => setSel(e.path)} onDoubleClick={() => open(e)}
                      style={{ background: sel === e.path ? "var(--accent)" : i % 2 ? "rgba(255,255,255,0.035)" : "transparent", cursor: "default" }}>
                      <td style={{ padding: "4px 8px", borderRadius: "6px 0 0 6px" }}><div className="row" style={{ gap: 8 }}><FileIcon e={e} size={18} /><span className="ellipsis">{e.name}</span></div></td>
                      <td style={{ opacity: 0.7 }}>{fmtDate(e.modified)}</td>
                      <td style={{ opacity: 0.7 }}>{e.is_dir ? "--" : fmtSize(e.size)}</td>
                      <td style={{ opacity: 0.7, borderRadius: "0 6px 6px 0" }}>{e.kind}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <div className="row" style={{ height: "100%", alignItems: "stretch" }}>
                {chain.map((d, ci) => (
                  <div key={d} className="col" style={{ width: 220, flex: "none", overflowY: "auto", padding: 6, borderRight: "1px solid rgba(255,255,255,0.08)" }}>
                    {(cols[d] ?? []).map((e) => {
                      const on = chain[ci + 1] === e.path || sel === e.path;
                      return (
                        <button key={e.path} className="row" onClick={() => (e.is_dir ? go(e.path) : setSel(e.path))} onDoubleClick={() => open(e)}
                          style={{ gap: 7, padding: "3px 8px", borderRadius: 6, fontSize: 13, background: on ? "var(--accent)" : "transparent" }}>
                          <FileIcon e={e} size={16} /><span className="ellipsis grow" style={{ textAlign: "left" }}>{e.name}</span>
                          {e.is_dir && <Icon name="right" size={12} style={{ opacity: 0.6 }} />}
                        </button>
                      );
                    })}
                  </div>
                ))}
                {selected && !selected.is_dir && (
                  <div className="col" style={{ width: 240, flex: "none", alignItems: "center", padding: 20, gap: 8, fontSize: 12 }}>
                    <FileIcon e={selected} size={96} />
                    <b style={{ fontSize: 14, textAlign: "center", wordBreak: "break-word" }}>{selected.name}</b>
                    <span style={{ opacity: 0.6 }}>{selected.kind} · {fmtSize(selected.size)}</span>
                    <span style={{ opacity: 0.6 }}>Modified {fmtDate(selected.modified)}</span>
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Path bar */}
          <div className="row" style={{ height: 28, padding: "0 14px", gap: 4, fontSize: 11, color: "var(--ink-2)", borderTop: "1px solid rgba(255,255,255,0.07)" }}>
            {chain.map((d, i) => (
              <span key={d} className="row" style={{ gap: 4 }}>
                {i > 0 && <Icon name="right" size={10} style={{ opacity: 0.5 }} />}
                <button onClick={() => go(d)} className="row" style={{ gap: 4 }}><AppIcon role="folder" size={13} />{d === home ? "Home" : base(d)}</button>
              </span>
            ))}
            <span className="grow" />
            <span>{shown.length} items{path !== "/" && <> · <button onClick={() => go(parent(path))}>Enclosing folder</button></>}</span>
          </div>
        </div>
      </div>
    </div>
  );
}
