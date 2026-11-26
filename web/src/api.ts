import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export type App = { id: string; name: string; comment: string; exec: string; role: string; icon: string };
export type Pin = { id: string; name: string; role: string; cmd: string; running: number; focused: boolean; window: number | null };
export type Status = { battery: number | null; charging: boolean; volume: number | null; muted: boolean; brightness: number | null; wifi: string | null; online: boolean };
export type Media = { title: string; artist: string; playing: boolean; position_us: number | null; length_us: number | null };
export type Clip = { id: number; text: string; kind: "text" | "link" | "color" | "file"; category: string; pinned: boolean };
export type ShelfItem = { path: string; name: string; is_dir: boolean };
export type AgentOpts = { model?: string | null; effort?: string | null; mode?: string | null; session?: string | null; fresh?: boolean; cwd?: string | null };
export type Entry = { name: string; path: string; is_dir: boolean; size: number; modified: number; kind: string };

/** True when running inside Tauri (false in a plain browser preview). */
export const inTauri = typeof (window as any).__TAURI_INTERNALS__ !== "undefined";

async function call<T>(cmd: string, args?: Record<string, unknown>, fallback?: T): Promise<T> {
  if (!inTauri) return fallback as T;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    console.warn(cmd, e);
    return fallback as T;
  }
}

export const api = {
  surfaceResize: (width: number, height: number) => call("surface_resize", { width, height }),
  surfaceInput: (rects: number[][]) => call("surface_input", { rects }),
  surfaceHide: () => call("surface_hide"),
  surfaceGlass: (boxes: number[][], preset: string) => call("surface_glass", { boxes, preset }),
  surfaceToggle: (label: string) => call("surface_toggle", { label }),
  apps: () => call<App[]>("apps", {}, []),
  dockPins: () => call<Pin[]>("dock_pins", {}, []),
  launch: (cmd: string) => call("launch", { cmd }),
  focusWindow: (id: number) => call("focus_window", { id }),
  status: () => call<Status | null>("status", {}, null),
  setVolume: (pct: number) => call("set_volume", { pct: Math.round(pct) }),
  setBrightness: (pct: number) => call("set_brightness", { pct: Math.round(pct) }),
  media: () => call<Media | null>("media", {}, null),
  mediaControl: (action: string, position_us?: number) => call("media_control", { action, positionUs: position_us ?? null }),
  clipboard: () => call<Clip[]>("clipboard", {}, []),
  copyText: (text: string) => call("copy_text", { text }),
  shelf: () => call<ShelfItem[]>("shelf", {}, []),
  shelfAdd: (path: string) => call("shelf_add", { path }),
  shelfRemove: (path: string) => call("shelf_remove", { path }),
  listDir: (path: string, hidden: boolean) => call<Entry[]>("list_dir", { path, hidden }, []),
  openPath: (path: string) => call("open_path", { path }),
  homeDir: () => call<string>("home_dir", {}, "/home"),
  findFiles: (query: string) => call<Entry[]>("find_files", { query }, []),
  ptySpawn: (id: number, cmd: string | null, cwd: string | null, cols: number, rows: number) =>
    call("pty_spawn", { id, cmd, cwd, cols, rows }),
  ptyWrite: (id: number, data: string) => call("pty_write", { id, data }),
  ptyResize: (id: number, cols: number, rows: number) => call("pty_resize", { id, cols, rows }),
  ptyKill: (id: number) => call("pty_kill", { id }),
  harnesses: () => call<string[]>("harnesses", {}, []),
  agentSend: (key: string, harness: string, prompt: string, opts: AgentOpts) =>
    inTauri ? invoke<void>("agent_send", { key, harness, prompt, opts }) : Promise.reject(new Error("Agents need the Solyra desktop")),
  agentStop: (key: string) => call("agent_stop", { key }),
  windowAction: (action: "close" | "minimize" | "maximize" | "drag") => call("window_action", { action }),
};

export function on<T>(event: string, f: (payload: T) => void): () => void {
  if (!inTauri) return () => {};
  let un: UnlistenFn | null = null;
  let dead = false;
  listen<T>(event, (e) => f(e.payload)).then((u) => (dead ? u() : (un = u)));
  return () => {
    dead = true;
    un?.();
  };
}

/** Poll `f` every `ms` while mounted. */
export function poll(f: () => void, ms: number): () => void {
  f();
  const t = window.setInterval(f, ms);
  return () => window.clearInterval(t);
}

export const fmtTime = (us: number | null | undefined) => {
  if (us == null || us < 0) return "--:--";
  const s = Math.floor(us / 1e6);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

const lastShape: Record<string, string> = {};
// A surface shown again gets a new wl_surface: send its shapes afresh.
on("surface-shown", () => { delete lastShape.input; delete lastShape.glass; });
/** Send this surface's input region and glass boxes when they change. */
export function surfaceShape(input: (Element | null | undefined)[], glass: [Element | null | undefined, number][], preset: string, pad = 0) {
  const rect = (el: Element) => el.getBoundingClientRect();
  const inRects = input.filter(Boolean).map((el) => { const r = rect(el!); return [Math.floor(r.left) - pad, Math.floor(r.top) - pad, Math.ceil(r.width) + 2 * pad, Math.ceil(r.height) + 2 * pad]; }).filter((r) => r[2] > 0 && r[3] > 0);
  const boxes = glass.filter(([el]) => el).map(([el, rad]) => { const r = rect(el!); return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height), rad]; }).filter((b) => b[2] > 0 && b[3] > 0);
  const a = JSON.stringify(inRects), b = JSON.stringify(boxes) + preset;
  if (lastShape.input !== a) { lastShape.input = a; api.surfaceInput(inRects); }
  if (lastShape.glass !== b) { lastShape.glass = b; api.surfaceGlass(boxes, preset); }
}
