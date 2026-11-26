//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland
import Quickshell.Wayland

// Solyra's desktop shell on Hyprland: menu bar, dock and notch on every
// screen; Spotlight, Control Center, notifications, snap layouts and the
// lock screen once. Keybinds arrive as Hyprland global shortcuts
// (solyra:spotlight …) and everything is scriptable with `qs ipc call shell …`.
ShellRoot {
    id: shell
    property bool dnd: false

    function spotlight(q) { if (spot.open && !q) spot.hide(); else spot.show(q || "") }
    function control() { cc.open = !cc.open }
    function lock() { lockScreen.lock() }
    function snap() { snaps.open = !snaps.open }
    function toggleOverview() { overviewWin.open = !overviewWin.open }
    function applyArrange() {
        // Auto-arrange on: windows tile (dwindle) as they open; off: they float
        // like macOS and are centred.
        for (const t of Hyprland.toplevels.values) {
            const addr = "address:0x" + t.address
            Hyprland.dispatch("hl.dsp.window.float({ action = '" + (Theme.autoArrange ? "disable" : "enable") + "', window = '" + addr + "' })")
            if (!Theme.autoArrange) Hyprland.dispatch("hl.dsp.window.center({ window = '" + addr + "' })")
        }
    }
    function menuAction(a) {
        if (a.startsWith("key:")) { Quickshell.execDetached(["wtype", "-M", "ctrl", a.slice(-1), "-m", "ctrl"]); return }
        // Hyprland 0.57 dispatchers are Lua expressions.
        const d = {
            close: "hl.dsp.window.close()",
            fullscreen: "hl.dsp.window.fullscreen()",
            maximize: "hl.dsp.window.fullscreen({ mode = 'maximized' })",
            minimize: "hl.dsp.window.move({ workspace = 'special:minimized', follow = false })",
            float: "hl.dsp.window.float({ action = 'toggle' })",
            hide: "hl.dsp.window.move({ workspace = 'special:minimized', follow = false })"
        }
        if (a === "overview") { toggleOverview(); return }
        if (d[a]) Hyprland.dispatch(d[a])
        else if (a === "lock") lock()
        else if (a === "snap") snap()
        else if (a === "arrange") { Theme.autoArrange = !Theme.autoArrange; Theme.save(); applyArrange() }
        else if (a === "settings") Quickshell.execDetached(["solyra-settings"])
        else if (a === "new") Quickshell.execDetached(["solyra-term"])
        else if (a === "logout") Hyprland.dispatch("hl.dsp.exit()")
        else if (a === "about") Quickshell.execDetached(["notify-send", "Solyra", "A macOS-style desktop on Hyprland"])
        else if (a === "help") Quickshell.execDetached(["notify-send", "Shortcuts", "Super+Space Spotlight · Super+Z Snap · Super+L Lock · Super+C Control Center"])
    }

    // New windows float and centre unless auto-arrange is on.
    Connections {
        target: Hyprland
        function onRawEvent(ev) {
            if (ev.name === "openwindow" && !Theme.autoArrange) {
                const addr = "address:0x" + ev.data.split(",")[0]
                Hyprland.dispatch("hl.dsp.window.float({ action = 'enable', window = '" + addr + "' })")
                Hyprland.dispatch("hl.dsp.window.center({ window = '" + addr + "' })")
            }
        }
    }

    Variants {
        model: Quickshell.screens
        Scope {
            id: perScreen
            required property var modelData
            Wallpaper { screenModel: perScreen.modelData }
            Bar { screenModel: perScreen.modelData; shell: shell }
            Dock { screenModel: perScreen.modelData; shell: shell }
            Island { id: island; screenModel: perScreen.modelData }
            Connections {
                target: ipc
                function onIslandRequested(tab) { if (tab === "close") island.collapse(); else island.expand(tab) }
                function onHudRequested(kind, value) { island.hud(kind, value) }
            }
        }
    }
    Spotlight { id: spot; shell: shell }
    ControlCenter { id: cc; shell: shell }
    Notifications { shell: shell }
    SnapLayouts { id: snaps; shell: shell }
    Overview { id: overviewWin }
    Lock { id: lockScreen }

    GlobalShortcut { appid: "solyra"; name: "spotlight"; onPressed: shell.spotlight() }
    GlobalShortcut { appid: "solyra"; name: "control"; onPressed: shell.control() }
    GlobalShortcut { appid: "solyra"; name: "lock"; onPressed: shell.lock() }
    GlobalShortcut { appid: "solyra"; name: "snap"; onPressed: shell.snap() }
    GlobalShortcut { appid: "solyra"; name: "island"; onPressed: ipc.islandRequested("media") }
    GlobalShortcut { appid: "solyra"; name: "overview"; onPressed: shell.toggleOverview() }

    IpcHandler {
        id: ipc
        target: "shell"
        signal islandRequested(string tab)
        signal hudRequested(string kind, real value)
        function spotlight(): void { shell.spotlight("") }
        function query(text: string): void { shell.spotlight(text) }
        function control(): void { shell.control() }
        function lock(): void { shell.lock() }
        function snap(): void { shell.snap() }
        function overview(): void { shell.toggleOverview() }
        function island(tab: string): void { islandRequested(tab) }
        function hud(kind: string, value: real): void { hudRequested(kind, value) }
        function glass(value: int): void { Theme.glass = value; Theme.save() }
        function arrange(on: bool): void { Theme.autoArrange = on; Theme.save(); shell.applyArrange() }
    }
}
