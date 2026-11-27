//@ pragma UseQApplication
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Hyprland
import Quickshell.Wayland

// Solyra's desktop shell on Hyprland: menu bar, dock and the Dynamic Island
// (which is also Spotlight) on every screen; Control Center, notifications,
// snap layouts, Mission Control, Floating File Basket, and the 3-mode Lock
// Screen once. Keybinds arrive as Hyprland global shortcuts (solyra:spotlight …)
// and everything is scriptable with `qs ipc call shell …`.
ShellRoot {
    id: root
    property bool dnd: false
    // Shared countdown timer (also driven by the Pomodoro Focus/Break dial).
    property real timerEnd: 0
    property int timerDuration: 1500
    property string pomodoroPhase: "focus" // "focus" | "break"
    property int pomodoroMinutes: 25
    property bool pomodoroSound: true
    property bool pomodoroAutoBreak: false

    // Shared Next Calendar Event & persistent local events list (~/.local/share/solyra/events.json).
    property var calendarEvents: [
        { title: "Launch review", time: "10:00 – 11:00 AM", accent: "#8e82ff", bg: "#201c3a" },
        { title: "Lunch with Sam", time: "12:30 – 1:45 PM", accent: "#ff9f0a", bg: "#2e1f11" }
    ]
    property string nextEventTitle: "Design review"
    property real nextEventEnd: 0

    // Live Coding Agent activity detector (auto-detects claude/codex/cargo/makepkg or IPC override).
    property bool agentActive: false
    property string agentLabel: "Coding Agent"
    property bool agentPinned: false

    // Inline Reply-from-Island notification state.
    property var replyBanner: null
    property string lastReplyLog: ""

    // Floating File Basket overlay state (shared across screens).
    property bool basketOpen: false

    // Shared Lock Screen mode: "depth" (Full Art & Clock Depth) | "lyrics" (Live Lyrics) | "queue" (Playing Next).
    property string lockMode: "depth"

    // Shared Playing Next Queue (auto-populated from ~/Music + active track context).
    property var mediaQueue: [
        { title: "Moonlit Water", artist: "Clear Sky", duration: "3:18", path: "", art: "", c1: "#2563eb", c2: "#06b6d4" },
        { title: "Gnossienne No. 1", artist: "Erik Satie", duration: "3:44", path: "", art: "", c1: "#f59e0b", c2: "#b45309" },
        { title: "Clair de Lune", artist: "Claude Debussy", duration: "5:02", path: "", art: "", c1: "#6366f1", c2: "#312e81" },
        { title: "Nocturne Op. 9 No. 2", artist: "Frédéric Chopin", duration: "4:31", path: "", art: "", c1: "#ec4899", c2: "#701a75" }
    ]

    // Notification Center: open notifications, newest first.
    property bool centerOpen: false
    property var history: []
    property alias lyrics: lyricsSvc
    LyricsService { id: lyricsSvc }
    function remember(note) { history = [note].concat(history.filter(x => x !== note)).slice(0, 100) }
    function forget(note) { history = history.filter(x => x !== note) }
    function clearHistory() { for (const note of history.slice()) note.dismiss(); history = [] }
    function notificationCenter() { centerOpen = !centerOpen; if (centerOpen) cc.open = false }
    signal spotlightRequested(string query)

    // Persistent calendar events
    FileView {
        id: eventsFile
        path: Theme.home + "/.local/share/solyra/events.json"
        onLoaded: {
            try {
                const parsed = JSON.parse(text())
                if (Array.isArray(parsed) && parsed.length > 0) root.calendarEvents = parsed
            } catch (e) {}
        }
    }
    function addCalendarEvent(title, timeRange) {
        if (!title || !title.trim()) return
        const palette = [
            { accent: "#8e82ff", bg: "#201c3a" },
            { accent: "#ff9f0a", bg: "#2e1f11" },
            { accent: "#30d158", bg: "#14291c" },
            { accent: "#0a84ff", bg: "#12233b" }
        ]
        const p = palette[calendarEvents.length % palette.length]
        const next = calendarEvents.concat([{
            title: title.trim(),
            time: (timeRange && timeRange.trim()) ? timeRange.trim() : "Today",
            accent: p.accent,
            bg: p.bg
        }])
        calendarEvents = next
        Quickshell.execDetached([
            "sh", "-c",
            "mkdir -p \"$HOME/.local/share/solyra\" && printf '%s' \"$1\" > \"$HOME/.local/share/solyra/events.json\"",
            "sh", JSON.stringify(next, null, 2)
        ])
    }

    // Discover real local audio files in ~/Music to enrich the Playing Next queue
    Process {
        id: musicScanProc
        running: true
        command: ["sh", "-c", "find \"$HOME/Music\" -maxdepth 2 -type f \\( -iname '*.mp3' -o -iname '*.flac' -o -iname '*.ogg' -o -iname '*.m4a' -o -iname '*.opus' \\) 2>/dev/null | head -12"]
        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.trim().split("\n").filter(x => x.length > 0)
                if (lines.length === 0) return
                const cols = [["#2563eb", "#06b6d4"], ["#f59e0b", "#b45309"], ["#6366f1", "#312e81"], ["#ec4899", "#701a75"]]
                const out = []
                for (let i = 0; i < lines.length; i++) {
                    const p = lines[i]
                    const base = p.split("/").pop().replace(/\.[^.]+$/, "")
                    const parts = base.split(" - ")
                    const c = cols[i % cols.length]
                    out.push({
                        title: parts.length > 1 ? parts[1] : base,
                        artist: parts.length > 1 ? parts[0] : "Local Library",
                        duration: "3:30",
                        path: p,
                        art: "",
                        c1: c[0],
                        c2: c[1]
                    })
                }
                root.mediaQueue = out
            }
        }
    }

    function playQueueItem(idx) {
        if (idx < 0 || idx >= mediaQueue.length) return
        const item = mediaQueue[idx]
        if (item.path) {
            Quickshell.execDetached(["sh", "-c", "pkill -x mpv 2>/dev/null; mpv --no-video \"$1\" >/dev/null 2>&1 &", "sh", item.path])
        } else {
            Quickshell.execDetached(["playerctl", "next"])
        }
        // Rotate selected item to top or remove from up-next
        const rest = mediaQueue.slice(idx + 1).concat(mediaQueue.slice(0, idx))
        mediaQueue = rest.concat([item])
    }

    // Lightweight coding-agent live process detector
    Process {
        id: agentDetectProc
        command: ["sh", "-c", "pgrep -x 'claude|codex|cargo|makepkg|rustc' >/dev/null 2>&1 && echo 1 || echo 0"]
        stdout: StdioCollector {
            onStreamFinished: {
                if (!root.agentPinned) root.agentActive = text.trim() === "1"
            }
        }
    }
    Timer { interval: 4500; repeat: true; running: true; onTriggered: if (!root.agentPinned) agentDetectProc.running = true }

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
    function timer(seconds) {
        if (seconds > 0) {
            timerDuration = seconds
            timerEnd = Date.now() + seconds * 1000
        } else {
            timerEnd = 0
        }
    }
    function startPomodoro(minutes, phase) {
        pomodoroPhase = phase || pomodoroPhase
        if (minutes > 0) pomodoroMinutes = minutes
        timer(pomodoroMinutes * 60)
    }
    function triggerReplyBanner(sender, message, appName) {
        replyBanner = {
            sender: sender || "Message",
            message: message || "",
            app: appName || "Messages",
            ts: Date.now()
        }
        ipc.replyRequested()
    }
    function sendIslandReply(text) {
        if (!text || !text.trim() || !replyBanner) return
        const entry = "[" + Qt.formatDateTime(new Date(), "HH:mm") + "] To " + replyBanner.sender + ": " + text.trim()
        lastReplyLog = entry
        Quickshell.execDetached([
            "sh", "-c",
            "mkdir -p \"$HOME/.local/share/solyra\" && printf '%s\\n' \"$1\" >> \"$HOME/.local/share/solyra/replies.log\"",
            "sh", entry
        ])
        replyBanner = null
        ipc.toastRequested("Replied|" + text.trim())
    }
    function control() { cc.open = !cc.open }
    function lock() { lockScreen.lock() }
    function setLockMode(m) {
        if (m === "depth" || m === "lyrics" || m === "queue") lockMode = m
    }
    function snap() { snaps.open = !snaps.open }
    function toggleOverview() { overviewWin.open = !overviewWin.open }
    function setArrange(on) {
        Theme.autoArrange = on
        Theme.save()
        reloadTimer.restart()
    }
    function applyArrange() {
        for (const t of Hyprland.toplevels.values) {
            const addr = "address:0x" + t.address
            Hyprland.dispatch("hl.dsp.window.float({ action = '" + (Theme.autoArrange ? "disable" : "enable") + "', window = '" + addr + "' })")
            if (!Theme.autoArrange) Hyprland.dispatch("hl.dsp.window.center({ window = '" + addr + "' })")
        }
    }
    function menuAction(a) {
        if (a.startsWith("key:")) { Quickshell.execDetached(["wtype", "-M", "ctrl", a.slice(-1), "-m", "ctrl"]); return }
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
        else if (a === "spotlight") spotlight("")
        else if (a === "go:home") Quickshell.execDetached(["solyra-files", Theme.home])
        else if (a === "go:docs") Quickshell.execDetached(["solyra-files", Theme.home + "/Documents"])
        else if (a === "go:downloads") Quickshell.execDetached(["solyra-files", Theme.home + "/Downloads"])
        else if (a === "logout") Hyprland.dispatch("hl.dsp.exit()")
        else if (a === "about") Quickshell.execDetached(["notify-send", "Solyra", "A macOS-style desktop on Hyprland"])
        else if (a === "help") Quickshell.execDetached(["notify-send", "Shortcuts", "Super+Space Spotlight (in the island) · Super+Z Snap · Super+L Lock · Super+C Control Center"])
    }

    ShelfModel { id: sharedShelf }

    Variants {
        model: Quickshell.screens
        Scope {
            id: perScreen
            required property var modelData
            Wallpaper { screenModel: perScreen.modelData }
            Bar { screenModel: perScreen.modelData; shell: root }
            Dock { id: dock; screenModel: perScreen.modelData; shell: root }
            DesktopWidgets { screenModel: perScreen.modelData }
            HotCorners { screenModel: perScreen.modelData; shell: root }
            Island { id: island; screenModel: perScreen.modelData; shell: root; shelf: sharedShelf }
            Connections {
                target: ipc
                function onIslandRequested(tab) { if (tab === "close") island.collapse(); else island.expand(tab) }
                function onHudRequested(arg) {
                    const parts = arg.split(":")
                    const kind = parts[0] || ""
                    const val = parts.length > 1 ? parseFloat(parts[1]) : 0
                    const lbl = parts.length > 2 ? parts.slice(2).join(":") : ""
                    island.hud(kind, val, lbl)
                }
                function onDownloadsRequested() {
                    const focused = Hyprland.focusedMonitor
                    if (!focused || focused.name === perScreen.modelData.name) dock.downloadsStackToggled()
                }
                function onDockHoverRequested(x) {
                    const focused = Hyprland.focusedMonitor
                    if (!focused || focused.name === perScreen.modelData.name) dock.forcedMx = x
                }
                function onPeekRequested(on) { island.peekOpen = on }
                function onReplyRequested() { island.openReply() }
                function onToastRequested(payload) {
                    const i = payload.indexOf("|")
                    island.showToast(i < 0 ? payload : payload.slice(0, i), i < 0 ? "" : payload.slice(i + 1))
                }
                function onShelfActionRequested(act) {
                    if (act === "compress") island.triggerShelfCompress()
                    else if (act === "selectAll") island.triggerShelfSelectFirst()
                }
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
    FloatingBasket { id: basket; shell: root; model: sharedShelf }
    Lock { id: lockScreen; shell: root; lyrics: root.lyrics }

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
        signal downloadsRequested()
        signal dockHoverRequested(real x)
        signal peekRequested(bool on)
        signal replyRequested()
        signal toastRequested(string payload)
        signal shelfActionRequested(string act)
        function spotlight(): void { root.spotlight("") }
        function query(text: string): void { root.spotlight(text) }
        function control(): void { root.control() }
        function lock(): void { root.lock() }
        function lockMode(mode: string): void { root.setLockMode(mode) }
        function snap(): void { root.snap() }
        function overview(): void { root.toggleOverview() }
        function island(tab: string): void { islandRequested(tab) }
        function downloads(): void { downloadsRequested() }
        function dockHover(x: real): void { dockHoverRequested(x) }
        function hud(kind: string, value: real): void { hudRequested(kind + ":" + value) }
        function hudLabel(kind: string, value: real, label: string): void { hudRequested(kind + ":" + value + ":" + label) }
        function timer(seconds: int): void { root.timer(seconds) }
        function pomodoro(minutes: int): void { root.startPomodoro(minutes, "focus") }
        function triActivity(on: bool): void {
            root.agentPinned = on
            root.agentActive = on
            root.nextEventEnd = on ? (Date.now() + 299 * 1000) : 0
            root.timer(on ? 1500 : 0)
        }
        function hoverPeek(on: bool): void { peekRequested(on) }
        function reply(sender: string, message: string): void { root.triggerReplyBanner(sender, message, "Messages") }
        function shelfCompress(): void { shelfActionRequested("compress") }
        function shelfSelect(): void { shelfActionRequested("selectAll") }
        function shelfAdd(path: string): void { sharedShelf.add("file://" + path, false) }
        function basket(): void { root.basketOpen = !root.basketOpen }
        function record(): void { root.toggleRecording() }
        function notifications(): void { root.notificationCenter() }
        function search(text: string): void { root.spotlight(text) }
        function glass(value: int): void { Theme.glass = value; Theme.save() }
        function arrange(on: bool): void { root.setArrange(on) }
    }
}
