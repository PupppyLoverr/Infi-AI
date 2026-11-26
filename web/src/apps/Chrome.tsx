import { useState } from "react";
import { api } from "../api";

/** macOS traffic lights: 12 px, 8 px apart; glyphs appear on group hover. */
export function TrafficLights() {
  const [hot, setHot] = useState(false);
  const dot = (bg: string, glyph: string, action: "close" | "minimize" | "maximize") => (
    <button onClick={(e) => { e.stopPropagation(); api.windowAction(action); }}
      style={{ width: 12, height: 12, borderRadius: 6, background: bg, boxShadow: "inset 0 0 0 0.5px rgba(0,0,0,0.25)", display: "grid", placeItems: "center", fontSize: 9, lineHeight: 1, fontWeight: 800, color: "rgba(0,0,0,0.55)" }}>
      {hot ? glyph : ""}
    </button>
  );
  return (
    <div className="row" style={{ gap: 8 }} onMouseEnter={() => setHot(true)} onMouseLeave={() => setHot(false)}>
      {dot("#ff5f57", "×", "close")}
      {dot("#febc2e", "−", "minimize")}
      {dot("#28c840", "+", "maximize")}
    </div>
  );
}

/** Drag the window from empty titlebar space (not from its buttons). */
export const dragProps = {
  onMouseDown: (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const t = e.target as HTMLElement;
    if (t.closest("button, input, textarea, [data-nodrag]")) return;
    api.windowAction("drag");
  },
  onDoubleClick: (e: React.MouseEvent) => {
    const t = e.target as HTMLElement;
    if (!t.closest("button, input, textarea, [data-nodrag]")) api.windowAction("maximize");
  },
};
