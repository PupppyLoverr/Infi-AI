import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, Pin, poll, surfaceShape } from "../api";
import { AppIcon } from "../Icon";

// macOS 27 / pearOS dock: 48 px icons on a 56 px pitch, glass capsule,
// Gaussian magnification (max 1.75x, sigma 1.4 cells), glass tooltip pills,
// a small dot under running apps, then a divider, Downloads and Trash.
const CELL = 48;
const GAP = 8;
const PAD = 12;
const MAX_SCALE = 1.75;
const SIGMA = 1.4;

type Item =
  | { kind: "launchpad" }
  | { kind: "app"; pin: Pin }
  | { kind: "sep" }
  | { kind: "downloads" }
  | { kind: "trash" };

const label = (it: Item) =>
  it.kind === "app" ? it.pin.name : it.kind === "launchpad" ? "Launchpad" : it.kind === "downloads" ? "Downloads" : it.kind === "trash" ? "Trash" : "";

export function Dock() {
  const [pins, setPins] = useState<Pin[]>([]);
  const [mx, setMx] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [bounce, setBounce] = useState<string | null>(null);
  const card = useRef<HTMLDivElement>(null);

  useEffect(() => poll(() => api.dockPins().then(setPins), 1500), []);

  const items: Item[] = useMemo(() => {
    const files = pins.filter((p) => p.role === "files");
    const rest = pins.filter((p) => p.role !== "files");
    return [
      ...files.map((pin) => ({ kind: "app" as const, pin })),
      { kind: "launchpad" as const },
      ...rest.map((pin) => ({ kind: "app" as const, pin })),
      { kind: "sep" as const },
      { kind: "downloads" as const },
      { kind: "trash" as const },
    ];
  }, [pins]);

  // Resting centres (relative to the card's left edge) drive the Gaussian.
  const centres = useMemo(() => {
    let x = PAD;
    return items.map((it) => {
      const w = it.kind === "sep" ? 9 : CELL;
      const c = x + w / 2;
      x += w + GAP;
      return c;
    });
  }, [items]);

  const scales = items.map((it, i) => {
    if (it.kind === "sep" || mx == null) return 1;
    const d = (mx - centres[i]) / (CELL + GAP);
    return 1 + (MAX_SCALE - 1) * Math.exp(-(d * d) / (2 * SIGMA * SIGMA));
  });

  // Only the card (and, while hovering, the room the icons grow into)
  // takes the pointer; the rest of the surface lets clicks through.
  const tip = useRef<HTMLDivElement>(null);
  const room = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    surfaceShape([card.current, mx != null ? room.current : null], [[card.current, 24], [tip.current, 14]], "dock");
  });

  const activate = (it: Item) => {
    if (it.kind === "app") {
      if (it.pin.window != null) api.focusWindow(it.pin.window);
      else if (it.pin.cmd) {
        setBounce(it.pin.id);
        window.setTimeout(() => setBounce(null), 900);
        api.launch(it.pin.cmd);
      }
    } else if (it.kind === "launchpad") api.surfaceToggle("spotlight");
    else if (it.kind === "downloads") api.launch('solyra-web files "$HOME/Downloads"');
    else if (it.kind === "trash") api.launch('solyra-web files "$HOME/.local/share/Trash/files"');
  };

  return (
    <div style={{ height: "100%", display: "flex", alignItems: "flex-end", justifyContent: "center", paddingBottom: 2, position: "relative" }}>
      {/* While magnifying, the band above the card keeps the pointer too. */}
      <div ref={room} style={{ position: "absolute", left: 0, right: 0, bottom: 72, height: 70, pointerEvents: "none" }} />
      <div
        ref={card}
        className="glass"
        onMouseMove={(e) => {
          const r = card.current!.getBoundingClientRect();
          setMx(e.clientX - r.left);
        }}
        onMouseLeave={() => {
          setMx(null);
          setHover(null);
        }}
        style={{
          display: "flex", alignItems: "flex-end", gap: GAP, padding: `0 ${PAD}px`,
          height: 70, borderRadius: 24, transition: "width 120ms ease-out",
        }}
      >
        {items.map((it, i) => {
          if (it.kind === "sep")
            return <div key={i} style={{ width: 1, alignSelf: "center", height: 40, margin: "0 4px", background: "rgba(255,255,255,0.28)" }} />;
          const s = scales[i];
          const size = Math.round(CELL * s);
          const role = it.kind === "app" ? it.pin.role || "overview" : it.kind;
          const running = it.kind === "app" && it.pin.running > 0;
          const bouncing = it.kind === "app" && bounce === it.pin.id;
          return (
            <button
              key={i}
              onMouseEnter={() => setHover(i)}
              onMouseLeave={() => setHover((h) => (h === i ? null : h))}
              onClick={() => activate(it)}
              style={{ position: "relative", width: size, height: size, marginBottom: 11, transition: "width 90ms, height 90ms" }}
            >
              <div style={{ animation: bouncing ? "dock-bounce 0.9s ease-out" : undefined }}>
                <AppIcon role={role} size={size} />
              </div>
              {hover === i && (
                <div ref={tip} className="glass tooltip" style={{ position: "absolute", bottom: size + 12, left: "50%", transform: "translateX(-50%)", padding: "5px 12px", fontSize: 13, fontWeight: 500, whiteSpace: "nowrap" }}>
                  {label(it)}
                </div>
              )}
              {running && (
                <div style={{ position: "absolute", bottom: -8, left: "50%", width: 4, height: 4, marginLeft: -2, borderRadius: 4, background: "rgba(255,255,255,0.85)", boxShadow: "0 0 4px rgba(255,255,255,0.6)" }} />
              )}
            </button>
          );
        })}
      </div>
      <style>{`@keyframes dock-bounce { 0%,100% { transform: translateY(0) } 30% { transform: translateY(-18px) } 60% { transform: translateY(0) } 75% { transform: translateY(-6px) } }`}</style>
    </div>
  );
}
