import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, Clip, fmtTime, inTauri, Media, on, poll, ShelfItem, surfaceShape } from "../api";
import { Icon } from "../Icon";

// Droppy-style notch: pure black at the camera, flush with the top edge,
// with the small concave "ears" of a MacBook notch. Hover opens it into one
// continuous black shape (no separate card); a detached mode pill floats
// underneath (Media / Shelf / Clipboard). Compact: live activity wings.
type Mode = "media" | "shelf" | "clipboard";

const IDLE = { w: 190, h: 32, r: 12 };
const OPEN = { w: 560, h: 180, r: 30 };

export function Island() {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<Mode>("media");
  const [media, setMedia] = useState<Media | null>(null);
  const [shelf, setShelf] = useState<ShelfItem[]>([]);
  const [clips, setClips] = useState<Clip[]>([]);
  const [dropping, setDropping] = useState(false);
  const enterT = useRef<number>();
  const leaveT = useRef<number>();
  const shape = useRef<HTMLDivElement>(null);
  const pillRef = useRef<HTMLDivElement>(null);

  useEffect(() => poll(() => api.media().then(setMedia), 1000), []);
  useEffect(() => poll(() => api.shelf().then(setShelf), 2500), []);
  useEffect(() => poll(() => api.clipboard().then(setClips), 2000), []);
  useEffect(() => on("island-open", () => setOpen((o) => !o)), []);
  useEffect(() => on("island-shelf", () => { setOpen(true); setMode("shelf"); }), []);
  useEffect(() => on("island-clipboard", () => { setOpen(true); setMode("clipboard"); }), []);

  // Files dragged onto the notch go to the shelf (Tauri gives real paths).
  useEffect(() => {
    if (!inTauri) return;
    let un: (() => void) | undefined;
    import("@tauri-apps/api/webview").then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent((e) => {
          const p = e.payload as any;
          if (p.type === "over" || p.type === "enter") {
            setDropping(true);
            setOpen(true);
            setMode("shelf");
          } else if (p.type === "drop") {
            setDropping(false);
            (p.paths as string[]).forEach((x) => api.shelfAdd(x));
            window.setTimeout(() => api.shelf().then(setShelf), 200);
          } else setDropping(false);
        })
        .then((u) => (un = u)),
    );
    return () => un?.();
  }, []);

  const live = !!media && (media.playing || media.title !== "");
  const compact = live && !open ? { w: 300, h: 32, r: 12 } : null;
  const s = open ? OPEN : compact ?? IDLE;

  // The notch is opaque black: it takes the pointer (with a little slack so
  // hovering just beside it opens it) but needs no glass.
  useLayoutEffect(() => surfaceShape([shape.current, pillRef.current], [], "island", 10));

  const enter = () => {
    window.clearTimeout(leaveT.current);
    enterT.current = window.setTimeout(() => setOpen(true), 220);
  };
  const leave = () => {
    window.clearTimeout(enterT.current);
    leaveT.current = window.setTimeout(() => setOpen(false), 450);
  };

  const pct = media?.length_us ? Math.min(1, Math.max(0, (media.position_us ?? 0) / media.length_us)) : 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", height: "100%" }}>
      <div
        ref={shape}
        onMouseEnter={enter}
        onMouseLeave={leave}
        className="notch-black"
        style={{
          position: "relative", width: s.w, height: s.h, borderRadius: `0 0 ${s.r}px ${s.r}px`,
          transition: "width 380ms cubic-bezier(.2,.9,.25,1), height 380ms cubic-bezier(.2,.9,.25,1), border-radius 380ms",
          boxShadow: open ? "0 18px 40px rgba(0,0,0,0.45)" : "none", overflow: "hidden",
        }}
      >
        {/* The concave ears where the notch meets the screen edge. */}
        <Ear side="left" />
        <Ear side="right" />
        {!open && compact && media && (
          <div className="row" style={{ height: 32, padding: "0 12px", justifyContent: "space-between" }}>
            <div className="row" style={{ gap: 8 }}>
              <Art size={20} radius={5} />
              <span className="ellipsis" style={{ maxWidth: 70, fontSize: 12, fontWeight: 600 }}>{media.title || "Playing"}</span>
            </div>
            <Eq playing={media.playing} />
          </div>
        )}
        {open && (
          <div style={{ padding: "16px 22px 18px", height: "100%", opacity: 1, animation: "island-fade 260ms ease-out" }}>
            {mode === "media" && <MediaView media={media} pct={pct} />}
            {mode === "shelf" && <ShelfView items={shelf} dropping={dropping} onChange={() => api.shelf().then(setShelf)} />}
            {mode === "clipboard" && <ClipView clips={clips} />}
          </div>
        )}
      </div>
      {open && (
        <div ref={pillRef} onMouseEnter={enter} onMouseLeave={leave} className="notch-black row pill" style={{ marginTop: 8, padding: 4, gap: 2, animation: "island-fade 260ms ease-out" }}>
          {(["media", "shelf", "clipboard"] as Mode[]).map((m) => (
            <button key={m} onClick={() => setMode(m)} className="pill row"
              style={{ width: 34, height: 26, justifyContent: "center", background: mode === m ? "rgba(255,255,255,0.18)" : "transparent", color: mode === m ? "#fff" : "rgba(255,255,255,0.6)" }}>
              <Icon name={m === "media" ? "music" : m === "shelf" ? "tray" : "clipboard"} size={15} />
            </button>
          ))}
        </div>
      )}
      <style>{`@keyframes island-fade { from { opacity: 0; transform: translateY(-4px) } to { opacity: 1; transform: none } }
        @keyframes eq { 0%,100% { transform: scaleY(.35) } 50% { transform: scaleY(1) } }`}</style>
    </div>
  );
}

function Ear({ side }: { side: "left" | "right" }) {
  const r = 10;
  const style: React.CSSProperties = { position: "absolute", top: 0, width: r, height: r, [side]: -r } as any;
  const grad = side === "left" ? `radial-gradient(circle at 0 100%, transparent ${r - 0.5}px, #000 ${r}px)` : `radial-gradient(circle at 100% 100%, transparent ${r - 0.5}px, #000 ${r}px)`;
  return <div style={{ ...style, background: grad }} />;
}

function Art({ size, radius }: { size: number; radius: number }) {
  return (
    <div style={{ width: size, height: size, borderRadius: radius, flex: "none", display: "grid", placeItems: "center", background: "linear-gradient(135deg,#ff9f43,#ee5253 55%,#5f27cd)" }}>
      <Icon name="music" size={Math.round(size * 0.55)} style={{ color: "rgba(255,255,255,0.9)" }} />
    </div>
  );
}

function Eq({ playing }: { playing: boolean }) {
  return (
    <div className="row" style={{ gap: 2, height: 14 }}>
      {[0, 1, 2, 3, 4].map((i) => (
        <div key={i} style={{ width: 3, height: 14, borderRadius: 2, background: "#ff9f43", transformOrigin: "50% 100%",
          animation: playing ? `eq ${0.6 + i * 0.13}s ${i * 0.07}s ease-in-out infinite` : undefined, transform: playing ? undefined : "scaleY(.3)" }} />
      ))}
    </div>
  );
}

function MediaView({ media, pct }: { media: Media | null; pct: number }) {
  if (!media) return <Empty icon="music" text="Nothing is playing" />;
  return (
    <div className="col" style={{ gap: 12 }}>
      <div className="row" style={{ gap: 14 }}>
        <Art size={58} radius={12} />
        <div className="col grow" style={{ gap: 3 }}>
          <div className="ellipsis" style={{ fontSize: 16, fontWeight: 650 }}>{media.title || "Unknown title"}</div>
          <div className="ellipsis muted" style={{ fontSize: 13 }}>{media.artist}</div>
        </div>
        <Eq playing={media.playing} />
      </div>
      <div className="row" style={{ gap: 10, fontSize: 11, color: "rgba(255,255,255,0.6)" }}>
        <span style={{ width: 34 }}>{fmtTime(media.position_us)}</span>
        <div className="grow" style={{ height: 5, borderRadius: 4, background: "rgba(255,255,255,0.18)", position: "relative" }}
          onClick={(e) => {
            const r = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
            if (media.length_us) api.mediaControl("seek", Math.round(((e.clientX - r.left) / r.width) * media.length_us));
          }}>
          <div style={{ width: `${pct * 100}%`, height: "100%", borderRadius: 4, background: "rgba(255,255,255,0.92)" }} />
        </div>
        <span style={{ width: 34, textAlign: "right" }}>{media.length_us ? "-" + fmtTime(media.length_us - (media.position_us ?? 0)) : "--:--"}</span>
      </div>
      <div className="row" style={{ justifyContent: "center", gap: 34 }}>
        <button onClick={() => api.mediaControl("previous")}><Icon name="prev" size={22} /></button>
        <button onClick={() => api.mediaControl("play-pause")}><Icon name={media.playing ? "pause" : "play"} size={28} /></button>
        <button onClick={() => api.mediaControl("next")}><Icon name="next" size={22} /></button>
      </div>
    </div>
  );
}

function ShelfView({ items, dropping, onChange }: { items: ShelfItem[]; dropping: boolean; onChange: () => void }) {
  if (items.length === 0 || dropping)
    return (
      <div className="col" style={{ height: "100%", alignItems: "center", justifyContent: "center", gap: 8, borderRadius: 18, border: `1.5px dashed ${dropping ? "#0a84ff" : "rgba(255,255,255,0.22)"}`, color: dropping ? "#0a84ff" : "rgba(255,255,255,0.55)" }}>
        <Icon name="tray" size={26} />
        <span style={{ fontSize: 13 }}>{dropping ? "Drop to park on the shelf" : "Drop files here to keep them handy"}</span>
      </div>
    );
  return (
    <div className="row" style={{ gap: 12, height: "100%", alignItems: "flex-start", overflowX: "auto" }}>
      {items.map((f) => (
        <div key={f.path} className="col" style={{ width: 84, alignItems: "center", gap: 6, padding: 8, borderRadius: 14, background: "rgba(255,255,255,0.06)" }}
          onDoubleClick={() => api.openPath(f.path)} title={f.path}>
          <img src={f.is_dir ? "/icons/folder.png" : "/icons/notes.png"} width={44} height={44} alt="" />
          <span className="ellipsis" style={{ fontSize: 11, maxWidth: 76 }}>{f.name}</span>
          <div className="row" style={{ gap: 6, color: "rgba(255,255,255,0.55)" }}>
            <button title="Open" onClick={() => api.openPath(f.path)}><Icon name="eye" size={13} /></button>
            <button title="Copy path" onClick={() => api.copyText(f.path)}><Icon name="copy" size={13} /></button>
            <button title="Remove" onClick={() => api.shelfRemove(f.path).then(onChange)}><Icon name="x" size={13} /></button>
          </div>
        </div>
      ))}
    </div>
  );
}

function ClipView({ clips }: { clips: Clip[] }) {
  if (clips.length === 0) return <Empty icon="clipboard" text="Copy something and it appears here" />;
  return (
    <div className="row" style={{ gap: 10, height: "100%", overflowX: "auto" }}>
      {clips.slice(0, 12).map((c) => (
        <button key={c.id} onClick={() => api.copyText(c.text)} title="Copy again"
          className="col" style={{ width: 150, flex: "none", height: "100%", borderRadius: 14, overflow: "hidden", textAlign: "left", background: c.kind === "color" ? c.text : "rgba(255,255,255,0.07)" }}>
          {c.kind === "color" ? (
            <div className="col" style={{ height: "100%", justifyContent: "center", alignItems: "center", color: "#111", fontWeight: 700 }}>{c.text.toUpperCase()}</div>
          ) : (
            <>
              <div className="row" style={{ gap: 6, padding: "8px 10px 4px", fontSize: 11, color: "rgba(255,255,255,0.55)" }}>
                <Icon name={c.kind === "link" ? "link" : c.kind === "file" ? "document" : "clipboard"} size={12} />
                <span>{c.kind === "link" ? "Link" : c.kind === "file" ? "File" : "Text"}</span>
                {c.pinned && <Icon name="star" size={11} />}
              </div>
              <div style={{ padding: "0 10px", fontSize: 12, lineHeight: 1.35, overflow: "hidden", flex: 1, wordBreak: "break-word" }}>{c.text.slice(0, 160)}</div>
              <div style={{ padding: "4px 10px 8px", fontSize: 10, color: "rgba(255,255,255,0.4)" }}>{c.text.length} characters</div>
            </>
          )}
        </button>
      ))}
    </div>
  );
}

function Empty({ icon, text }: { icon: string; text: string }) {
  return (
    <div className="col" style={{ height: "100%", alignItems: "center", justifyContent: "center", gap: 8, color: "rgba(255,255,255,0.5)" }}>
      <Icon name={icon} size={24} />
      <span style={{ fontSize: 13 }}>{text}</span>
    </div>
  );
}
