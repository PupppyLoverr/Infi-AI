# macOS and Windows: what Solyra takes from each

Notes from studying both systems' desktop surfaces. No Apple or Microsoft
artwork is in this repository; everything Solyra draws is its own.

| Area | macOS | Windows 11 | Solyra |
| --- | --- | --- | --- |
| Top bar | Global menu bar: app menus left, status right; translucent (fully clear since Liquid Glass) | None; everything lives in the taskbar | Menu bar *or* compact *or* hidden. The focused app's name and real Go / Window / View / Help menus driven by the compositor, not by app cooperation (no global-menu protocol needed) |
| Dock / taskbar | Floating dock, optional magnification, running dots, auto-hide | Full-width taskbar, centred icons, Start, search, tray, clock | Four variants: Solyra card, Floating (magnify), Taskbar, Minimal; auto-hide for all |
| Live activity | Notch (hardware) on MacBooks; Dynamic Island on iPhone | Notification toasts, no live area | Notch or Island at the top centre, used for things that need the person: agent approvals and questions answered in place, now playing, notifications, agent progress |
| Quick settings | Control Center: grouped modules | Quick Settings: tiles and sliders | Control centre with only controls that act on the real system (no fake Wi-Fi or Bluetooth toggles) |
| Launcher | Spotlight: type first, one list | Start: pinned grid plus search | Spotlight-like, type first; grouped results (apps, windows, actions, ask an agent) |
| Lock screen | Large time top-centre, avatar and field below, wallpaper visible | Large clock bottom-left over the wallpaper; any key reveals sign-in | Both, plus a blended default (blurred wallpaper, centred) |
| Materials | Vibrancy, now Liquid Glass: adapts to the brightness behind it | Mica (long-lived, tinted by the wallpaper) and Acrylic (blur + tint + luminosity + noise, for transient surfaces) | Solid, See-through, Glass, Acrylic per surface; adaptive density so text keeps contrast; one switch for solid everywhere |
| Settings | Sidebar of panes, grouped rows | Sidebar of categories, cards | Sidebar of pages, every option with the original Linux tool one click away |
| Window chrome | Traffic lights, titlebar integrated with toolbar | Caption buttons right, Snap layouts | Server-side minimalism: rounded corners, focus border, no titlebar furniture; layout presets via keys and the Window menu |

What was deliberately *not* copied: the global-menu dependency on app
cooperation, Start's ad-like "recommended" area, and translucency without a
legibility guard. Solyra adds what neither has: a place where agents ask and
the person answers, with the exact action, resource and reversibility shown.

Sources: Microsoft Learn, "Materials in Windows 11" and "Acrylic material"
(the acrylic recipe: blur, exclusion blend, tint, luminosity, noise; acrylic
for transient surfaces). Apple newsroom, June 2025 (Liquid Glass: fully
transparent menu bar, layered Dock, colour informed by surrounding content)
and WWDC25 session 310 (the material adapts to the brightness of the content
behind it).
