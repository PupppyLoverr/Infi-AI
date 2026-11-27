//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland
import Quickshell.Wayland

// Solyra's desktop shell on Hyprland: menu bar, dock and the Dynamic Island
// (which is also Spotlight) on every screen; Control Center, notifications,
// snap layouts, Mission Control and the lock screen once. Keybinds arrive as Hyprland global shortcuts
// (solyra:spotlight …) and everything is scriptable with `qs ipc call shell …`.
ShellRoot {
    // Called `root`, not `shell`: in `Island { shell: shell }` the right-hand
    // name would resolve to the island's own (empty) `shell` property.
    id: root
    property bool dnd: false
    // The island's countdown timer (a live activity), shared by every screen.
    property real timerEnd: 0
    // Notification Center: open notifications, newest first.
    property bool centerOpen: false
    property var history: []
    function remember(note) { history = [note].concat(history.filter(x => x !== note)).slice(0, 100) }
    function forget(note) { history = history.filter(x => x !== note) }
    function clearHistory() { for (const note of history.slice()) note.dismiss(); history = [] }
    function notificationCenter() { centerOpen = !centerOpen; if (centerOpen) cc.open = false }
    signal spotlightRequested(string query)

    // Night Shift: hyprsunset warms the display while it runs.
    Process { running: Theme.nightShift; command: ["hyprsunset", "-t", "4500"] }

    // Screen recording (wf-recorder): a red live activity in the island while
    // it runs; the file lands in ~/Videos and a notification says where.
    property bool recording: false
    property real recordStart: 0
    property string recordFile: ""
    function toggleRecording() {
        if (recording) { recProc.signal(2); return }
        recordFile = Theme.home + "/Videos/Recording " + Qt.formatDateTime(new Date(), "yyyy-MM-dd 'at' HH.mm.ss") + ".mp4"
        recProc.command = ["sh", "-c", "mkdir -p \"$HOME/Videos\" && exec wf-recorder -f \"$1\"", "sh", recordFile]
        recordStart = Date.now()
        recording = true
        recProc.running = true
    }
    Process {
        id: recProc
        onExited: (code) => {
            root.recording = false
            Quickshell.execDetached(["notify-send", "-a", "Screen Recording", code === 0 || code === 130 || code === 2 ? "Recording saved" : "Recording stopped",
                                     code === 127 ? "wf-recorder is not installed." : root.recordFile])
        }
    }

    // Spotlight is the Dynamic Island: the one on the focused screen morphs
    // into the search field.
    function spotlight(q) { spotlightRequested(q || "") }
    function timer(seconds) { timerEnd = seconds > 0 ? Date.now() + seconds * 1000 : 0 }
    function control() { cc.open = !cc.open }
    function lock() { lockScreen.lock() }
    function snap() { snaps.open = !snaps.open }
    function toggleOverview() { overviewWin.open = !overviewWin.open }
    // Auto-arrange is a Hyprland window rule (hyprland.lua reads look.json):
    // save, reload Hyprland so new windows follow it, re-arrange open ones.
    function setArrange(on) {
        Theme.autoArrange = on
        Theme.save()
        reloadTimer.restart()
    }
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
        else if (a === "arrange") setArrange(!Theme.autoArrange)
        else if (a === "settings") Quickshell.execDetached(["solyra-settings"])
        else if (a === "new") Quickshell.execDetached(["solyra-term"])
        else if (a === "logout") Hyprland.dispatch("hl.dsp.exit()")
        else if (a === "about") Quickshell.execDetached(["notify-send", "Solyra", "A macOS-style desktop on Hyprland"])
        else if (a === "help") Quickshell.execDetached(["notify-send", "Shortcuts", "Super+Space Spotlight (in the island) · Super+Z Snap · Super+L Lock · Super+C Control Center"])
    }

    Variants {
        model: Quickshell.screens
        Scope {
            id: perScreen
            required property var modelData
            Wallpaper { screenModel: perScreen.modelData }
            Bar { screenModel: perScreen.modelData; shell: root }
            Dock { screenModel: perScreen.modelData; shell: root }
            DesktopWidgets { screenModel: perScreen.modelData }
            HotCorners { screenModel: perScreen.modelData; shell: root }
            Island { id: island; screenModel: perScreen.modelData; shell: root }
            Connections {
                target: ipc
                function onIslandRequested(tab) { if (tab === "close") island.collapse(); else island.expand(tab) }
                // "kind:value" (IPC signals carry one argument).
                function onHudRequested(arg) { const i = arg.indexOf(":"); island.hud(i < 0 ? arg : arg.slice(0, i), i < 0 ? 0 : parseFloat(arg.slice(i + 1))) }
            }
            Connections {
                target: root
                function onSpotlightRequested(q) {
                    const focused = Hyprland.focusedMonitor
                    if (!focused || focused.name === perScreen.modelData.name) island.toggleSearch(q)
                }
            }
        }
    }
    ControlCenter { id: cc; shell: root }
    Notifications { shell: root }
    NotificationCenter { shell: root }
    SnapLayouts { id: snaps; shell: root }
    Overview { id: overviewWin }
    Lock { id: lockScreen }

    // After look.json is written: Hyprland re-reads its rules, then the
    // windows already open move to match.
    Timer { id: reloadTimer; interval: 250; onTriggered: { Quickshell.execDetached(["hyprctl", "reload"]); arrangeTimer.restart() } }
    Timer { id: arrangeTimer; interval: 600; onTriggered: root.applyArrange() }

    GlobalShortcut { appid: "solyra"; name: "spotlight"; onPressed: root.spotlight() }
    GlobalShortcut { appid: "solyra"; name: "control"; onPressed: root.control() }
    GlobalShortcut { appid: "solyra"; name: "lock"; onPressed: root.lock() }
    GlobalShortcut { appid: "solyra"; name: "snap"; onPressed: root.snap() }
    GlobalShortcut { appid: "solyra"; name: "island"; onPressed: ipc.islandRequested("media") }
    GlobalShortcut { appid: "solyra"; name: "overview"; onPressed: root.toggleOverview() }

    IpcHandler {
        id: ipc
        target: "shell"
        signal islandRequested(string tab)
        signal hudRequested(string arg)
        function spotlight(): void { root.spotlight("") }
        function query(text: string): void { root.spotlight(text) }
        function control(): void { root.control() }
        function lock(): void { root.lock() }
        function snap(): void { root.snap() }
        function overview(): void { root.toggleOverview() }
        function island(tab: string): void { islandRequested(tab) }
        function hud(kind: string, value: real): void { hudRequested(kind + ":" + value) }
        function timer(seconds: int): void { root.timer(seconds) }
        function record(): void { root.toggleRecording() }
        function notifications(): void { root.notificationCenter() }
        function search(text: string): void { root.spotlight(text) }
        function glass(value: int): void { Theme.glass = value; Theme.save() }
        function arrange(on: bool): void { root.setArrange(on) }
    }
}
