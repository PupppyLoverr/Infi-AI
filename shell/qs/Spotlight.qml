import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

// Spotlight, as macOS 26/27 draws it: Super+Space springs a dark smoke-glass
// capsule out of the Dynamic Island to just below the menu bar, "Search or
// Ask" in large type with a microphone on the right. Under it, a separate
// frosted panel: with nothing typed, the four places to look (Applications
// ⌃1, Files ⌃2, Actions ⌃3, Clipboard ⌃4); typing turns it into results
// (Top Hit, apps, files, calculator, clipboard, actions, Ask). Escape or a
// click outside closes it. The microphone shows only when a local speech
// tool is installed (whisper.cpp's whisper-cli with a model), and dictates
// five seconds into the field.
PanelWindow {
    id: sp
    property var shell
    property bool open: false
    visible: open || capsule.opacity > 0.01
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-spotlight"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None
    mask: Region { item: open ? catcher : capsule }

    SearchModel {
        id: search
        shell: sp.shell
        clips: sp.clips
        onDone: sp.hide()
    }
    property var clips: []

    function show(q) {
        search.reset(q || "")
        field.text = q || ""
        arrowed = false
        open = true
        field.forceActiveFocus()
    }
    function hide() { open = false; search.stop() }
    function toggle(q) { if (open && !q) hide(); else show(q) }

    readonly property var categories: [
        ["Applications", "apps", "Apps"], ["Files", "folder", "Files"],
        ["Actions", "layers", "Actions"], ["Clipboard", "copies", "Clipboard"]]
    readonly property bool browsing: search.query.trim() === "" && search.filter === "All"
    // A category row lights only once the arrow keys are used (as the photo:
    // nothing is selected when Spotlight opens).
    property bool arrowed: false

    // Speech: whisper.cpp when present.
    property bool canDictate: false
    property bool listening: false
    Process {
        running: true
        command: ["sh", "-c", "command -v whisper-cli >/dev/null && command -v arecord >/dev/null && ls \"$HOME\"/.local/share/whisper/ggml-*.bin >/dev/null 2>&1 && echo yes"]
        stdout: StdioCollector { onStreamFinished: sp.canDictate = text.trim() === "yes" }
    }
    Process {
        id: dictate
        command: ["sh", "-c", "f=$(mktemp --suffix .wav); arecord -q -d 5 -f S16_LE -r 16000 -c 1 \"$f\" && whisper-cli -nt -np -m \"$(ls \"$HOME\"/.local/share/whisper/ggml-*.bin | head -1)\" -f \"$f\" 2>/dev/null; rm -f \"$f\""]
        stdout: StdioCollector { onStreamFinished: { const t = text.replace(/\s+/g, " ").trim(); if (t) field.text = t; sp.listening = false } }
    }

    // A click outside closes.
    MouseArea { id: catcher; anchors.fill: parent; onClicked: sp.hide() }

    // ------------------------------------------------------------- capsule
    Glass {
        id: capsule
        kind: "smoke"
        r: height / 2
        anchors.horizontalCenter: parent.horizontalCenter
        // Springs out of the island (a 100x24 pill at the top) to its place.
        width: sp.open ? 600 : 84
        height: sp.open ? 56 : 24
        y: sp.open ? 36 : 4
        opacity: sp.open ? 1 : 0
        Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on height { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on y { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on opacity { NumberAnimation { duration: Theme.ms(140) } }
        MouseArea { anchors.fill: parent }

        TextField {
            id: field
            anchors { left: parent.left; leftMargin: 24; right: mic.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
            opacity: capsule.width > 560 ? 1 : 0
            placeholderText: search.filter === "All" ? "Search or Ask"
                : search.filter === "Apps" ? "Search Applications"
                : search.filter === "Ask" ? "Ask anything" : "Search " + search.filter
            color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.55)
            selectionColor: Theme.accent; selectedTextColor: "white"
            font.family: Theme.font; font.pixelSize: 22; font.weight: Font.Normal
            background: null
            onTextChanged: search.query = text
            Keys.onPressed: ev => {
                if (ev.key === Qt.Key_Escape) { if (search.filter !== "All" && !text) search.filter = "All"; else sp.hide(); ev.accepted = true }
                else if ((ev.modifiers & Qt.ControlModifier) && ev.key >= Qt.Key_1 && ev.key <= Qt.Key_4) { search.filter = sp.categories[ev.key - Qt.Key_1][2]; search.sel = 0; ev.accepted = true }
                else if (ev.key === Qt.Key_Down) { sp.arrowed = true; search.move(1); list.positionViewAtIndex(search.sel, ListView.Contain); ev.accepted = true }
                else if (ev.key === Qt.Key_Up) { sp.arrowed = true; search.move(-1); list.positionViewAtIndex(search.sel, ListView.Contain); ev.accepted = true }
                else if (ev.key === Qt.Key_Tab) { search.cycleFilter(); ev.accepted = true }
                else if (ev.key === Qt.Key_Backspace && !text && search.filter !== "All") { search.filter = "All"; ev.accepted = true }
                else if (ev.key === Qt.Key_Return || ev.key === Qt.Key_Enter) {
                    if (sp.browsing) { search.filter = sp.categories[Math.max(0, Math.min(3, search.sel))][2]; search.sel = 0 }
                    else search.activate()
                    ev.accepted = true
                }
            }
        }
        // Dictation (only with a local speech model).
        Item {
            id: mic
            anchors { right: parent.right; rightMargin: 20; verticalCenter: parent.verticalCenter }
            width: sp.canDictate ? 26 : 0; height: 26
            visible: sp.canDictate
            Icon { anchors.centerIn: parent; name: "mic"; size: 20; color: sp.listening ? "#ff453a" : Qt.rgba(1, 1, 1, 0.85) }
            TapHandler { onTapped: if (!sp.listening) { sp.listening = true; dictate.running = true } }
        }
    }

    // ------------------------------------------------------------- panel
    Glass {
        id: panel
        kind: "frosted"
        r: 26
        anchors.horizontalCenter: parent.horizontalCenter
        width: 584
        y: capsule.y + capsule.height + 14
        readonly property int rowsH: sp.browsing ? 4 * 58 + 18
                                    : search.filter === "Ask" ? Math.min(360, answer.implicitHeight + 70)
                                    : Math.min(420, list.contentHeight + 20)
        height: rowsH
        visible: sp.open && (sp.browsing || search.filter === "Ask" || search.results.length > 0)
        opacity: visible ? 1 : 0
        scale: visible ? 1 : 0.96
        Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }
        Behavior on scale { NumberAnimation { duration: Theme.ms(220); easing.type: Easing.OutCubic } }
        Behavior on height { NumberAnimation { duration: Theme.ms(160); easing.type: Easing.OutCubic } }
        MouseArea { anchors.fill: parent }

        // Nothing typed: where to look.
        Column {
            visible: sp.browsing
            x: 10; y: 8; width: parent.width - 20
            Repeater {
                model: sp.categories
                Rectangle {
                    required property var modelData
                    required property int index
                    width: parent.width; height: 58; radius: 18
                    color: catHov.hovered || (sp.arrowed && search.sel === index) ? Qt.rgba(1, 1, 1, 0.12) : "transparent"
                    Icon { x: 18; anchors.verticalCenter: parent.verticalCenter; name: modelData[1]; size: 24 }
                    Text { x: 64; anchors.verticalCenter: parent.verticalCenter; text: modelData[0]; color: "white"; font.family: Theme.font; font.pixelSize: 18; font.weight: Font.DemiBold }
                    Text { anchors.right: parent.right; anchors.rightMargin: 18; anchors.verticalCenter: parent.verticalCenter; text: "⌃" + (index + 1); color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 15 }
                    HoverHandler { id: catHov; onHoveredChanged: if (hovered) search.sel = index }
                    TapHandler { onTapped: { search.filter = modelData[2]; search.sel = 0; field.forceActiveFocus() } }
                }
            }
        }

        // Ask: the answer.
        Column {
            visible: search.filter === "Ask"
            x: 20; y: 16; width: parent.width - 40; spacing: 10
            Row {
                spacing: 6
                Repeater {
                    model: [["claude", "Claude"], ["codex", "ChatGPT"]]
                    Rectangle {
                        required property var modelData
                        height: 26; width: pl.implicitWidth + 22; radius: 13
                        color: search.provider === modelData[0] ? Qt.rgba(1, 1, 1, 0.22) : Qt.rgba(1, 1, 1, 0.10)
                        Text { id: pl; anchors.centerIn: parent; text: modelData[1]; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium }
                        TapHandler { onTapped: search.provider = modelData[0] }
                    }
                }
                Text { anchors.verticalCenter: parent.verticalCenter; leftPadding: 8; text: search.asking ? "Thinking…" : (search.answer ? "" : "Press Return to ask"); color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12 }
            }
            Text { id: answer; width: parent.width; wrapMode: Text.Wrap; text: search.answer; color: "white"; font.family: Theme.font; font.pixelSize: 14; textFormat: Text.MarkdownText }
        }

        // Results.
        ListView {
            id: list
            visible: !sp.browsing && search.filter !== "Ask"
            x: 10; y: 10; width: parent.width - 20; height: parent.height - 20
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: search.results
            currentIndex: search.sel
            delegate: Column {
                required property var modelData
                required property int index
                width: list.width
                Text {
                    visible: index === 0 || search.results[index - 1].group !== modelData.group
                    height: visible ? 26 : 0
                    leftPadding: 14; topPadding: 6
                    text: modelData.group; color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                }
                Rectangle {
                    width: parent.width; height: modelData.group === "Top Hit" ? 56 : 44; radius: 14
                    color: index === search.sel ? Qt.rgba(1, 1, 1, 0.16) : "transparent"
                    Row {
                        anchors.fill: parent; anchors.leftMargin: 14; spacing: 12
                        Image {
                            visible: !!modelData.entry
                            anchors.verticalCenter: parent.verticalCenter
                            width: modelData.group === "Top Hit" ? 38 : 30; height: width
                            sourceSize: Qt.size(64, 64)
                            source: {
                                const e = modelData.entry
                                if (!e) return ""
                                const own = { "solyra-term": "terminal", "solyra-files": "files", "solyra-settings": "settings", "firefox": "browser", "mpv": "music" }
                                if (own[e.id]) return Qt.resolvedUrl("apps/" + own[e.id] + ".png")
                                const p = e.icon ? Quickshell.iconPath(e.icon, true) : ""
                                return p !== "" ? p : Qt.resolvedUrl("apps/overview.png")
                            }
                        }
                        Item {
                            visible: !modelData.entry
                            anchors.verticalCenter: parent.verticalCenter
                            width: 30; height: 30
                            Icon { anchors.centerIn: parent; name: modelData.icon || "info"; size: 20 }
                        }
                        Column {
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width - 70
                            Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: "white"; font.family: Theme.font; font.pixelSize: modelData.group === "Top Hit" ? 16 : 14; font.weight: Font.DemiBold }
                            Text { width: parent.width; elide: Text.ElideMiddle; text: modelData.sub; color: Qt.rgba(1, 1, 1, 0.55); font.family: Theme.font; font.pixelSize: 12 }
                        }
                    }
                    HoverHandler { onHoveredChanged: if (hovered) search.sel = index }
                    TapHandler { onTapped: search.activate(index) }
                }
            }
        }
    }
}
