import QtQuick
import Quickshell
import Quickshell.Io

// "Search or Ask": the search behind the Dynamic Island's Spotlight. Apps
// (desktop entries), files under home, a calculator, clipboard history,
// system actions, and Ask:
// the query goes to a signed-in agent CLI (Claude: `claude -p`, ChatGPT:
// `codex exec`) on the user's own subscription login, streamed back.
QtObject {
    id: m
    property var shell
    property string query: ""
    property string filter: "All"     // All | Apps | Files | Actions | Clipboard | Ask
    property int sel: 0
    property var files: []
    property var clips: []                // clipboard history, newest first
    property string provider: "claude"
    property string answer: ""
    property bool asking: false
    signal done()

    function reset(q) { query = q || ""; filter = "All"; sel = 0; answer = ""; asking = false }
    function stop() { m.askProc.running = false }
    function cycleFilter() { const f = ["All", "Apps", "Files", "Actions", "Clipboard", "Ask"]; filter = f[(f.indexOf(filter) + 1) % f.length] }
    function move(d) { sel = Math.max(0, Math.min(results.length - 1, sel + d)) }
    // Return: Ask asks; anything else runs and the island closes.
    function activate(i) {
        if (filter === "Ask") { ask(); return }
        const r = results[i === undefined ? sel : i]
        if (!r) return
        if (r.group === "Ask") { r.run(); return }
        r.run(); done()
    }

    function calc(q) {
        const s = q.replace(/\s+/g, "").replace(/×/g, "*").replace(/÷/g, "/").replace(/\^/g, "**")
        if (!/^[\d.+\-*/()%*]+$/.test(s) || !/\d/.test(s) || !/[+\-*/%]/.test(s)) return null
        try { const v = Function('"use strict";return (' + s + ')')(); return (typeof v === "number" && isFinite(v)) ? String(Math.round(v * 1e10) / 1e10) : null } catch (e) { return null }
    }
    function score(q, s) { s = (s || "").toLowerCase(); if (s.startsWith(q)) return 3; if (s.split(/[\s\-_.]/).some(w => w.startsWith(q))) return 2; return s.indexOf(q) >= 0 ? 1 : 0 }

    // System plumbing that ships desktop entries but is not an app a person
    // looks for (Spotlight on macOS shows none of these).
    readonly property var hidden: ({
        "qv4l2": true, "qvidcap": true, "footclient": true, "foot-server": true,
        "avahi-discover": true, "bssh": true, "bvnc": true, "lstopo": true,
        "org.freedesktop.IBus.Setup": true, "xgps": true, "xgpsspeed": true,
        "assistant": true, "designer": true, "linguist": true, "qdbusviewer": true,
        "cmake-gui": true, "electron36": true, "jconsole-java-openjdk": true,
        "jshell-java-openjdk": true, "nvtop": true, "htop": true, "btop": true, "vim": true
    })
    readonly property var actions: [
        { title: "Lock Screen", sub: "Lock this session", icon: "lock", run: () => m.shell.lock() },
        { title: "Control Center", sub: "Wi-Fi, sound, display, glass", icon: "settings", run: () => m.shell.control() },
        { title: "New Terminal", sub: "Open Solyra Terminal", icon: "terminal", run: () => Quickshell.execDetached(["solyra-term"]) },
        { title: "Snap Layouts", sub: "Arrange the focused window", icon: "grid", run: () => m.shell.snap() },
        { title: "Mission Control", sub: "Show all windows", icon: "desktop", run: () => m.shell.menuAction("overview") },
        { title: "Auto-arrange Windows", sub: "Tile windows as they open, or float them", icon: "grid", run: () => m.shell.menuAction("arrange") },
        { title: "Start a 5-minute Timer", sub: "A live activity in the Dynamic Island", icon: "clock", run: () => m.shell.timer(300) },
        { title: "Start a 25-minute Focus Timer", sub: "A live activity in the Dynamic Island", icon: "clock", run: () => m.shell.timer(1500) }
    ]

    readonly property var results: {
        const q = query.trim().toLowerCase()
        const out = []
        const want = f => filter === "All" || filter === f
        const c = calc(query)
        if (c !== null && filter === "All") out.push({ group: "Calculator", title: c, sub: query.trim() + " =", icon: "calculator", run: () => Quickshell.execDetached(["wl-copy", c]) })
        if (want("Apps")) {
            let apps = DesktopEntries.applications.values.filter(e => !e.noDisplay && !m.hidden[e.id])
            if (q) apps = apps.map(e => [Math.max(score(q, e.name) * 2, score(q, e.genericName), score(q, e.comment)), e]).filter(x => x[0] > 0).sort((a, b) => b[0] - a[0]).map(x => x[1])
            for (const e of apps.slice(0, filter === "Apps" ? 14 : (q ? 6 : 6)))
                out.push({ group: "Applications", title: e.name, sub: e.comment || e.genericName || "Application", entry: e, run: () => e.execute() })
        }
        if (want("Files")) for (const f of files.slice(0, filter === "Files" ? 14 : 5))
            out.push({ group: "Files", title: f.split("/").pop(), sub: f, icon: "document", run: () => Qt.openUrlExternally("file://" + f) })
        if ((q && filter === "All") || filter === "Clipboard") for (const t of clips.filter(t => !q || t.toLowerCase().indexOf(q) >= 0).slice(0, filter === "Clipboard" ? 14 : 3))
            out.push({ group: "Clipboard", title: t.replace(/\s+/g, " ").slice(0, 120), sub: "Copy again", icon: "clipboard", run: () => Quickshell.execDetached(["wl-copy", t]) })
        if (want("Actions") && filter !== "Clipboard") for (const a of actions.filter(a => !q || score(q, a.title) > 0 || score(q, a.sub) > 0).slice(0, filter === "Actions" ? 10 : (q ? 3 : 0)))
            out.push({ group: "Actions", title: a.title, sub: a.sub, icon: a.icon, run: a.run })
        if (q && filter === "All") out.push({ group: "Ask", title: "Ask " + (provider === "claude" ? "Claude" : "ChatGPT") + ": " + query.trim(), sub: "Uses your signed-in " + (provider === "claude" ? "Claude" : "ChatGPT") + " account", icon: "agent", run: () => m.ask() })
        if (out.length && q && out[0].group === "Applications") out[0].group = "Top Hit"
        return out
    }

    property Timer fileTimer: Timer { interval: 150; onTriggered: if (m.query.trim().length >= 2) { m.findProc.running = false; m.findProc.running = true } else m.files = [] }
    onQueryChanged: { sel = 0; m.fileTimer.restart() }
    property Process findProc: Process {
        command: ["sh", "-c", "find \"$HOME\" -maxdepth 4 -not -path '*/.*' -iname \"*$1*\" 2>/dev/null | head -20", "sh", m.query.trim()]
        stdout: StdioCollector { onStreamFinished: m.files = text.split("\n").filter(x => x) }
    }

    function ask() {
        if (!query.trim()) return
        filter = "Ask"; answer = ""; asking = true
        m.askProc.command = provider === "claude"
            ? ["claude", "-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", query.trim()]
            : ["codex", "exec", "--json", query.trim()]
        m.askProc.running = true
    }
    property Process askProc: Process {
        stdout: SplitParser {
            onRead: line => {
                try {
                    const ev = JSON.parse(line)
                    if (ev.type === "stream_event" && ev.event && ev.event.delta && ev.event.delta.type === "text_delta") m.answer += ev.event.delta.text
                    else if (ev.type === "result" && ev.result && !m.answer) m.answer = ev.result
                    else if (ev.type === "item.completed" && ev.item && ev.item.type === "agent_message") m.answer += ev.item.text
                } catch (e) {}
            }
        }
        stderr: StdioCollector { onStreamFinished: if (text.trim() && !m.answer) m.answer = text.trim() }
        onExited: (code, status) => {
            m.asking = false
            if (code === 127 || (code !== 0 && !m.answer)) m.answer = (m.provider === "claude" ? "Claude Code" : "Codex") + " is not signed in or not installed. Run `" + (m.provider === "claude" ? "claude" : "codex") + "` once in Terminal to sign in with your subscription."
        }
    }
}
