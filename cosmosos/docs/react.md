# Web/React apps — investigation findings

Status: **investigated, not shipped.** No claim of React/browser support
is made anywhere in CosmosOS.

## The honest requirement

"Running a React app on CosmosOS" means embedding a real web engine —
Chromium/Blink (CEF/wry/Electron), WebKitGTK, or Servo. A fake shell that
draws DOM-like pixels would violate the project's no-fake-functionality
rule, so this was evaluated as: what real engine could ship as a Wayland
client under our compositor?

## Viable real paths

| Engine | Reality check |
|---|---|
| **WebKitGTK** (gtk4 + `webkit2gtk-6`) | Real Wayland client; runs under cosmos-compositor like any other app. ~150MB+ of runtime deps in the image; WebKit JIT works on aarch64/x86_64. The heaviest realistic option that still works. |
| **wry** (Tauri's webview, `wry` crate) | Thin Rust wrapper over WebKitGTK (Linux). Same footprint; gives `WebView` as a real browser surface in a Wayland window. |
| **CEF / Electron** | Works on Wayland only through ozone-wayland; large and fragile under a non-mainstream compositor. Not recommended. |
| **Servo** | Real but incomplete; a Wayland port would itself be a project. Not recommended. |

## Compatibility verdict for CosmosOS

WebKitGTK/wry is the only path consistent with "real, never faked":
a React bundle genuinely rendered by a real engine in a real Wayland
window. It costs ~150–200MB of image space — significant against the
~3–4GB budget — and buys "web apps in a window", not a Cosmos-native
experience.

Recommendation: out of scope for the stock image; documented here as
`wry`-on-Wayland if web-app support becomes a goal. No code claims this
works until a real wry window renders under cosmos-compositor.
