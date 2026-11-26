import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import "@xterm/xterm/css/xterm.css";
import { Dock } from "./surfaces/Dock";
import { Island } from "./surfaces/Island";
import { Spotlight } from "./surfaces/Spotlight";
import { Control } from "./surfaces/Control";
import { Files } from "./apps/Files";
import { Terminal } from "./apps/Terminal";

const route = window.location.hash.replace("#", "").split("?")[0] || "dock";
const views: Record<string, () => JSX.Element> = {
  dock: Dock, island: Island, spotlight: Spotlight, control: Control, files: Files, terminal: Terminal,
};
const View = views[route] ?? Dock;
document.body.dataset.surface = route;
ReactDOM.createRoot(document.getElementById("root")!).render(<View />);
