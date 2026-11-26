import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

// Spotlight (Super+Space): "Search or Ask". A glass capsule; results drop
// into a glass sheet (Top Hit, Applications, Files, Calculator, Actions).
// Ask sends the query to a signed-in agent CLI (Claude: `claude -p`, ChatGPT:
// `codex exec`) using the user's own subscription login, streamed back.
PanelWindow {
    id: sp
    property var shell
    property bool open: false
    visible: open
    anchors { top: true; bottom: true; left: true; right: true }
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-spotlight"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    property string query: ""
    property string filter: "All"     // All | Apps | Files | Actions | Ask
    property int sel: 0
    property var files: []
    property string provider: "claude"
    property string answer: ""
    property bool asking: false

    function show(q) { open = true; query = q || ""; field.text = query; filter = "All"; sel = 0; answer = ""; field.forceActiveFocus() }
    function hide() { open = false; askProc.running = false }

    function calc(q) {
        const s = q.replace(/\s+/g, "").replace(/×/g, "*").replace(/÷/g, "/").replace(/\^/g, "**")
        if (!/^[\d.+\-*/()%*]+$/.test(s) || !/\d/.test(s) || !/[+\-*/%]/.test(s)) return null
        try { const v = Function('"use strict";return (' + s + ')')(); return (typeof v === "number" && isFinite(v)) ? String(Math.round(v * 1e10) / 1e10) : null } catch (e) { return null }
    }
    function score(q, s) { s = (s || "").toLowerCase(); if (s.startsWith(q)) return 3; if (s.split(/[\s\-_.]/).some(w => w.startsWith(q))) return 2; return s.indexOf(q) >= 0 ? 1 : 0 }

    readonly property var actions: [
        { title: "Lock Screen", sub: "Lock this session", icon: "lock", run: () => sp.shell.lock() },
        { title: "Control Center", sub: "Wi-Fi, sound, display, glass", icon: "settings", run: () => sp.shell.control() },
        { title: "New Terminal", sub: "Open Solyra Terminal", icon: "terminal", run: () => Quickshell.execDetached(["solyra-term"]) },
        { title: "Snap Layouts", sub: "Arrange the focused window", icon: "grid", run: () => sp.shell.snap() },
        { title: "Mission Control", sub: "Show all windows", icon: "desktop", run: () => sp.shell.menuAction("overview") }
    ]

    readonly property var results: {
        const q = query.trim().toLowerCase()
        const out = []
        const want = f => filter === "All" || filter === f
        const c = calc(query)
        if (c !== null && filter === "All") out.push({ group: "Calculator", title: c, sub: query.trim() + " =", icon: "calculator", run: () => Quickshell.execDetached(["wl-copy", c]) })
        if (want("Apps")) {
            let apps = DesktopEntries.applications.values.filter(e => !e.noDisplay)
            if (q) apps = apps.map(e => [Math.max(score(q, e.name) * 2, score(q, e.genericName), score(q, e.comment)), e]).filter(x => x[0] > 0).sort((a, b) => b[0] - a[0]).map(x => x[1])
            for (const e of apps.slice(0, filter === "Apps" ? 14 : (q ? 6 : 8)))
                out.push({ group: "Applications", title: e.name, sub: e.comment || e.genericName || "Application", entry: e, run: () => e.execute() })
        }
        if (want("Files")) for (const f of files.slice(0, filter === "Files" ? 14 : 5))
            out.push({ group: "Files", title: f.split("/").pop(), sub: f, icon: "document", run: () => Qt.openUrlExternally("file://" + f) })
        if (want("Actions")) for (const a of actions.filter(a => !q || score(q, a.title) > 0).slice(0, filter === "Actions" ? 10 : (q ? 3 : 0)))
            out.push({ group: "Actions", title: a.title, sub: a.sub, icon: a.icon, run: a.run })
        if (q && filter === "All") out.push({ group: "Ask", title: "Ask " + (provider === "claude" ? "Claude" : "ChatGPT") + ": " + query.trim(), sub: "Uses your signed-in " + (provider === "claude" ? "Claude" : "ChatGPT") + " account", icon: "agent", run: () => sp.ask() })
        if (out.length && q && out[0].group === "Applications") out[0].group = "Top Hit"
        return out
    }

    Timer { id: fileTimer; interval: 150; onTriggered: if (sp.query.trim().length >= 2) { findProc.running = false; findProc.running = true } else sp.files = [] }
    onQueryChanged: { sel = 0; fileTimer.restart() }
    Process {
        id: findProc
        command: ["sh", "-c", "find \"$HOME\" -maxdepth 4 -not -path '*/.*' -iname \"*$1*\" 2>/dev/null | head -20", "sh", sp.query.trim()]
        stdout: StdioCollector { onStreamFinished: sp.files = text.split("\n").filter(x => x) }
    }

    function ask() {
        filter = "Ask"; answer = ""; asking = true
        askProc.command = provider === "claude"
            ? ["claude", "-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", query.trim()]
            : ["codex", "exec", "--json", query.trim()]
        askProc.running = true
    }
    Process {
        id: askProc
        stdout: SplitParser {
            onRead: line => {
                try {
                    const ev = JSON.parse(line)
                    if (ev.type === "stream_event" && ev.event && ev.event.delta && ev.event.delta.type === "text_delta") sp.answer += ev.event.delta.text
                    else if (ev.type === "result" && ev.result && !sp.answer) sp.answer = ev.result
                    else if (ev.type === "item.completed" && ev.item && ev.item.type === "agent_message") sp.answer += ev.item.text
                } catch (e) {}
            }
        }
        stderr: StdioCollector { onStreamFinished: if (text.trim() && !sp.answer) sp.answer = text.trim() }
        onExited: (code, status) => {
            sp.asking = false
            if (code === 127 || (code !== 0 && !sp.answer)) sp.answer = (sp.provider === "claude" ? "Claude Code" : "Codex") + " is not signed in or not installed. Run `" + (sp.provider === "claude" ? "claude" : "codex") + "` once in Terminal to sign in with your subscription."
        }
    }

    MouseArea { anchors.fill: parent; onClicked: sp.hide() }

    Column {
        id: box
        width: 680
        anchors.horizontalCenter: parent.horizontalCenter
        y: parent.height * 0.18
        spacing: 10
        Glass {
            width: parent.width; height: 56; r: 28
            MouseArea { anchors.fill: parent }
            Row {
                anchors.fill: parent; anchors.leftMargin: 20; anchors.rightMargin: 10; spacing: 12
                Icon { anchors.verticalCenter: parent.verticalCenter; name: "search"; size: 22; color: Theme.ink2 }
                TextField {
                    id: field
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width - 34 - pills.width - 24
                    placeholderText: "Search or Ask"
                    color: Theme.ink; placeholderTextColor: Theme.ink3
                    font.family: Theme.font; font.pixelSize: 22
                    background: null
                    onTextChanged: sp.query = text
                    Keys.onPressed: ev => {
                        if (ev.key === Qt.Key_Escape) { sp.hide(); ev.accepted = true }
                        else if (ev.key === Qt.Key_Down) { sp.sel = Math.min(sp.results.length - 1, sp.sel + 1); ev.accepted = true }
                        else if (ev.key === Qt.Key_Up) { sp.sel = Math.max(0, sp.sel - 1); ev.accepted = true }
                        else if (ev.key === Qt.Key_Tab) { const f = ["All", "Apps", "Files", "Actions", "Ask"]; sp.filter = f[(f.indexOf(sp.filter) + 1) % f.length]; ev.accepted = true }
                        else if (ev.key === Qt.Key_Return || ev.key === Qt.Key_Enter) {
                            if (sp.filter === "Ask") sp.ask()
                            else if (sp.results[sp.sel]) { const r = sp.results[sp.sel]; if (r.group !== "Ask") sp.hide(); r.run() }
                            ev.accepted = true
                        }
                    }
                }
                Row {
                    id: pills
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 4
                    Repeater {
                        model: ["Apps", "Files", "Actions", "Ask"]
                        Rectangle {
                            required property string modelData
                            height: 26; width: lbl.implicitWidth + 22; radius: 13
                            color: sp.filter === modelData ? Theme.accent : Theme.fill
                            Text { id: lbl; anchors.centerIn: parent; text: modelData; color: sp.filter === modelData ? "white" : Theme.ink2; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
                            TapHandler { onTapped: { sp.filter = sp.filter === modelData ? "All" : modelData; field.forceActiveFocus() } }
                        }
                    }
                }
            }
        }
        // Results sheet / Ask answer.
        Glass {
            visible: sp.filter === "Ask" ? true : sp.results.length > 0
            width: parent.width; r: 24
            height: sp.filter === "Ask" ? Math.min(420, answerText.implicitHeight + 70) : Math.min(400, list.contentHeight + 16)
            MouseArea { anchors.fill: parent }
            Column {
                visible: sp.filter === "Ask"
                anchors.fill: parent; anchors.margins: 16; spacing: 10
                Row {
                    spacing: 6
                    Repeater {
                        model: [["claude", "Claude"], ["codex", "ChatGPT"]]
                        Pill { required property var modelData; height: 24; text: modelData[1]; on: sp.provider === modelData[0]; onClicked: sp.provider = modelData[0] }
                    }
                    Text { anchors.verticalCenter: parent.verticalCenter; leftPadding: 8; text: sp.asking ? "Thinking…" : (sp.answer ? "" : "Press Return to ask"); color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12 }
                }
                Text { id: answerText; width: parent.width; wrapMode: Text.Wrap; text: sp.answer; color: Theme.ink; font.family: Theme.font; font.pixelSize: 14; textFormat: Text.MarkdownText }
            }
            ListView {
                id: list
                visible: sp.filter !== "Ask"
                anchors.fill: parent; anchors.margins: 8
                clip: true
                model: sp.results
                currentIndex: sp.sel
                section.property: "group"
                delegate: Column {
                    required property var modelData
                    required property int index
                    width: list.width
                    Text {
                        visible: index === 0 || sp.results[index - 1].group !== modelData.group
                        height: visible ? 24 : 0
                        leftPadding: 12; topPadding: 6
                        text: modelData.group; color: Theme.ink3; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold
                    }
                    Rectangle {
                        width: parent.width; height: modelData.group === "Top Hit" ? 54 : 44; radius: 12
                        color: index === sp.sel ? Theme.accent : "transparent"
                        Row {
                            anchors.fill: parent; anchors.leftMargin: 12; spacing: 12
                            Image {
                                visible: !!modelData.entry
                                anchors.verticalCenter: parent.verticalCenter
                                width: modelData.group === "Top Hit" ? 40 : 30; height: width
                                sourceSize: Qt.size(64, 64)
                                // Our own app art first, then the icon theme, then a generic app.
                                source: {
                                    const e = modelData.entry
                                    if (!e) return ""
                                    const own = { "solyra-term": "terminal", "solyra-files": "files", "solyra-settings": "settings", "firefox": "browser", "mpv": "music" }
                                    if (own[e.id]) return Qt.resolvedUrl("apps/" + own[e.id] + ".png")
                                    const p = e.icon ? Quickshell.iconPath(e.icon, true) : ""
                                    return p !== "" ? p : Qt.resolvedUrl("apps/overview.png")
                                }
                            }
                            Rectangle {
                                visible: !modelData.entry
                                anchors.verticalCenter: parent.verticalCenter
                                width: 30; height: 30; radius: 8; color: Theme.fill
                                Icon { anchors.centerIn: parent; name: modelData.icon || "info"; size: 17; color: index === sp.sel ? "white" : Theme.ink }
                            }
                            Column {
                                anchors.verticalCenter: parent.verticalCenter
                                width: parent.width - 80
                                Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: index === sp.sel ? "white" : Theme.ink; font.family: Theme.font; font.pixelSize: modelData.group === "Top Hit" ? 16 : 14; font.weight: Font.Medium }
                                Text { width: parent.width; elide: Text.ElideMiddle; text: modelData.sub; color: index === sp.sel ? Qt.rgba(1, 1, 1, 0.8) : Theme.ink2; font.family: Theme.font; font.pixelSize: 12 }
                            }
                        }
                        HoverHandler { onHoveredChanged: if (hovered) sp.sel = index }
                        TapHandler { onTapped: { if (modelData.group !== "Ask") sp.hide(); modelData.run() } }
                    }
                }
            }
        }
    }
}
