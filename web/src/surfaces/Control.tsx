import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, Media, on, poll, Status, surfaceShape } from "../api";
import { Icon } from "../Icon";

// macOS 27 Control Centre: a bento grid of glass modules. Every value is
// real (sysfs, wpctl, brightnessctl, nmcli, MPRIS); when a reading is not
// available the module says so rather than inventing one.
export function Control() {
  const [st, setSt] = useState<Status | null>(null);
  const [media, setMedia] = useState<Media | null>(null);
  const [focus, setFocus] = useState(false);
  const [vol, setVol] = useState<number | null>(null);
  const [bri, setBri] = useState<number | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => poll(() => api.status().then((s) => { setSt(s); if (s) { setVol((v) => v ?? s.volume); setBri((b) => b ?? s.brightness); } }), 2000), []);
  useEffect(() => poll(() => api.media().then(setMedia), 1500), []);
  useEffect(() => on<{ shown: boolean }>("surface-shown", (e) => e.shown && api.status().then((s) => { setSt(s); setVol(s?.volume ?? null); setBri(s?.brightness ?? null); })), []);
  useLayoutEffect(() => surfaceShape([box.current], [[box.current, 28]], "regular"));

  return (
    <div style={{ height: "100%", padding: 8 }} onKeyDown={(e) => e.key === "Escape" && api.surfaceHide()} tabIndex={-1}>
      <div ref={box} className="glass" style={{ borderRadius: 28, padding: 12, display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10, width: 356 }}>
        <Tile style={{ gridRow: "span 2" }}>
          <Toggle glyph="wifi" on={!!st?.wifi} title="Wi-Fi" sub={st?.wifi ?? (st?.online ? "Wired" : "Not connected")} onClick={() => api.launch("nm-connection-editor || foot -e nmtui")} />
          <Toggle glyph="bluetooth" on={false} title="Bluetooth" sub="Settings" onClick={() => api.launch("blueman-manager")} />
          <Toggle glyph="airplay" on={st?.online ?? false} title="Network" sub={st?.online ? "Online" : "Offline"} onClick={() => api.launch("nm-connection-editor")} />
        </Tile>
        <Tile style={{ padding: 0 }}>
          <button onClick={() => setFocus(!focus)} className="row" style={{ gap: 10, padding: 12, width: "100%", height: "100%" }}>
            <Circle on={focus} glyph="moon" color="#5e5ce6" />
            <div className="col"><b style={{ fontSize: 13 }}>Focus</b><span style={{ fontSize: 11, opacity: 0.6 }}>{focus ? "On" : "Off"}</span></div>
          </button>
        </Tile>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
          <Tile style={{ display: "grid", placeItems: "center" }}>
            <button onClick={() => api.launch("solyra-panel lock")} title="Lock Screen"><Icon name="lock" size={22} /></button>
          </Tile>
          <Tile style={{ display: "grid", placeItems: "center" }}>
            <button onClick={() => api.launch("solyra-settings")} title="Settings"><Icon name="settings" size={22} /></button>
          </Tile>
        </div>
        <Slider label="Display" glyph="sun" value={bri} onChange={(v) => { setBri(v); api.setBrightness(v); }} />
        <Slider label="Sound" glyph={st?.muted ? "volumeMute" : "volume"} value={vol} onChange={(v) => { setVol(v); api.setVolume(v); }} />
        <Tile style={{ gridColumn: "span 2" }}>
          {media ? (
            <div className="row" style={{ gap: 12 }}>
              <div style={{ width: 44, height: 44, borderRadius: 10, display: "grid", placeItems: "center", background: "linear-gradient(135deg,#ff9f43,#ee5253 55%,#5f27cd)" }}><Icon name="music" size={22} /></div>
              <div className="col grow"><b className="ellipsis" style={{ fontSize: 13 }}>{media.title || "Unknown"}</b><span className="ellipsis" style={{ fontSize: 12, opacity: 0.6 }}>{media.artist}</span></div>
              <button onClick={() => api.mediaControl("previous")}><Icon name="prev" size={18} /></button>
              <button onClick={() => api.mediaControl("play-pause")}><Icon name={media.playing ? "pause" : "play"} size={22} /></button>
              <button onClick={() => api.mediaControl("next")}><Icon name="next" size={18} /></button>
            </div>
          ) : (
            <div className="row" style={{ gap: 10, opacity: 0.6, fontSize: 13 }}><Icon name="music" size={18} />Not Playing</div>
          )}
        </Tile>
        <Tile style={{ gridColumn: "span 2" }}>
          <div className="row" style={{ gap: 10, fontSize: 13 }}>
            <Icon name={st?.charging ? "charging" : "battery"} size={18} />
            <span className="grow">Battery</span>
            <span style={{ opacity: 0.7 }}>{st?.battery != null ? `${st.battery}%${st.charging ? " · charging" : ""}` : "No battery"}</span>
          </div>
        </Tile>
      </div>
    </div>
  );
}

function Tile({ children, style }: { children: React.ReactNode; style?: React.CSSProperties }) {
  return <div style={{ borderRadius: 20, padding: 12, background: "rgba(255,255,255,0.09)", boxShadow: "inset 0 0 0 0.5px rgba(255,255,255,0.18)", ...style }}>{children}</div>;
}

function Circle({ on, glyph, color = "var(--accent)" }: { on: boolean; glyph: string; color?: string }) {
  return (
    <div style={{ width: 32, height: 32, borderRadius: 16, flex: "none", display: "grid", placeItems: "center", background: on ? color : "rgba(255,255,255,0.16)" }}>
      <Icon name={glyph} size={17} />
    </div>
  );
}

function Toggle({ glyph, on, title, sub, onClick }: { glyph: string; on: boolean; title: string; sub: string; onClick: () => void }) {
  return (
    <button onClick={onClick} className="row" style={{ gap: 10, width: "100%", padding: "5px 0", textAlign: "left" }}>
      <Circle on={on} glyph={glyph} />
      <div className="col grow"><b style={{ fontSize: 13 }}>{title}</b><span className="ellipsis" style={{ fontSize: 11, opacity: 0.6 }}>{sub}</span></div>
    </button>
  );
}

function Slider({ label, glyph, value, onChange }: { label: string; glyph: string; value: number | null; onChange: (v: number) => void }) {
  const track = useRef<HTMLDivElement>(null);
  const set = (x: number) => {
    const r = track.current!.getBoundingClientRect();
    onChange(Math.round(Math.min(100, Math.max(0, ((x - r.left) / r.width) * 100))));
  };
  return (
    <Tile style={{ gridColumn: "span 2", padding: "10px 12px" }}>
      <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 7 }}>{label}</div>
      {value == null ? (
        <div style={{ fontSize: 12, opacity: 0.55 }}>Not available on this device</div>
      ) : (
        <div ref={track} style={{ position: "relative", height: 26, borderRadius: 13, background: "rgba(255,255,255,0.16)", overflow: "hidden", cursor: "pointer" }}
          onPointerDown={(e) => { (e.target as HTMLElement).setPointerCapture(e.pointerId); set(e.clientX); }}
          onPointerMove={(e) => e.buttons && set(e.clientX)}>
          <div style={{ position: "absolute", inset: 0, width: `${Math.max(13, value)}%`, borderRadius: 13, background: "rgba(255,255,255,0.92)" }} />
          <Icon name={glyph} size={15} style={{ position: "absolute", left: 7, top: 5.5, color: "#333" }} />
        </div>
      )}
    </Tile>
  );
}
