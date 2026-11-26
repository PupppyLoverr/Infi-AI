import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { api, on } from "../../api";

// One xterm.js instance per PTY, kept outside React so a pane survives
// being re-parented when the split tree changes (its element is moved,
// not rebuilt), and a single listener routes PTY output by id.
type Live = { term: XTerm; fit: FitAddon; el: HTMLDivElement; onExit?: () => void; onTitle?: (t: string) => void };
const LIVE = new Map<number, Live>();
let wired = false;

function wire() {
  if (wired) return;
  wired = true;
  on<{ id: number; data: string }>("pty-output", ({ id, data }) => LIVE.get(id)?.term.write(data));
  on<{ id: number }>("pty-exit", ({ id }) => LIVE.get(id)?.onExit?.());
}

export const THEME = {
  background: "#00000000",
  foreground: "#e8ece9",
  cursor: "#f2f2f2",
  cursorAccent: "#101614",
  selectionBackground: "rgba(255,255,255,0.22)",
  black: "#1d2021", red: "#ff6b6b", green: "#7ee787", yellow: "#f2cc60", blue: "#79b8ff", magenta: "#d2a8ff", cyan: "#76e3ea", white: "#e6edf3",
  brightBlack: "#6e7681", brightRed: "#ffa198", brightGreen: "#a5f3a0", brightYellow: "#f8e3a1", brightBlue: "#a5d6ff", brightMagenta: "#e2c5ff", brightCyan: "#b3f0ff", brightWhite: "#ffffff",
};

function create(id: number, cmd: string | null, cwd: string | null): Live {
  wire();
  const el = document.createElement("div");
  el.style.cssText = "position:absolute;inset:10px 6px 6px 12px;";
  const term = new XTerm({
    allowTransparency: true,
    fontFamily: '"JetBrains Mono", "DejaVu Sans Mono", monospace',
    fontSize: 13,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: "block",
    scrollback: 10000,
    theme: THEME,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown" || !e.ctrlKey || !e.shiftKey) return true;
    if (e.code === "KeyC") {
      const s = term.getSelection();
      if (s) api.copyText(s);
      return false;
    }
    if (e.code === "KeyV") {
      navigator.clipboard?.readText().then((t) => t && api.ptyWrite(id, t)).catch(() => {});
      return false;
    }
    // Leave the window's own shortcuts (tabs, splits) to the window.
    return !["KeyT", "KeyD", "KeyE", "KeyW", "BracketLeft", "BracketRight"].includes(e.code);
  });
  term.onData((d) => api.ptyWrite(id, d));
  term.onResize(({ cols, rows }) => api.ptyResize(id, cols, rows));
  const live: Live = { term, fit, el };
  term.onTitleChange((t) => live.onTitle?.(t));
  LIVE.set(id, live);
  // Spawn once the element has a size, so the shell starts at the right width.
  requestAnimationFrame(() => {
    try { fit.fit(); } catch {}
    api.ptySpawn(id, cmd, cwd, term.cols, term.rows);
  });
  return live;
}

export function disposePty(id: number) {
  const l = LIVE.get(id);
  if (!l) return;
  api.ptyKill(id);
  l.term.dispose();
  l.el.remove();
  LIVE.delete(id);
}

export function focusPty(id: number) {
  LIVE.get(id)?.term.focus();
}

export function XTermPane({ id, cmd, cwd, focused, onFocus, onExit, onTitle }: { id: number; cmd: string | null; cwd: string | null; focused: boolean; onFocus: () => void; onExit: () => void; onTitle?: (t: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let l = LIVE.get(id);
    const fresh = !l;
    if (!l) l = create(id, cmd, cwd);
    l.onExit = onExit;
    host.current!.appendChild(l.el);
    if (fresh) l.term.open(l.el);
    const ro = new ResizeObserver(() => { try { l!.fit.fit(); } catch {} });
    ro.observe(host.current!);
    return () => { ro.disconnect(); l!.el.remove(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  useEffect(() => {
    const l = LIVE.get(id);
    if (l) { l.onExit = onExit; l.onTitle = onTitle; }
  });
  useEffect(() => { if (focused) LIVE.get(id)?.term.focus(); }, [focused, id]);
  return <div ref={host} onMouseDown={onFocus} style={{ position: "absolute", inset: 0 }} />;
}
