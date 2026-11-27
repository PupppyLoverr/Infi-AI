import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire

// Control Center, as macOS 26/27 lays it out: no panel, separate clear-glass
// modules floating under the menu bar. Wi-Fi, Bluetooth and Airplane Mode
// pills (a white disc with the coloured symbol when on), a square Now Playing
// tile, two round buttons (Snap Layouts, Screen Recording), a round Dark Mode
// toggle beside a Focus pill, the Display and Sound sliders (thin tracks with
// a symbol at each end; Sound's output button lists outputs and per-app
// volume), and "Edit Controls" for the rest (Night Shift, accessibility,
// hot corners, widgets, the glass slider). Every reading is real:
// NetworkManager, BlueZ, rfkill, brightnessctl, PipeWire, MPRIS.
// Each module is its own glass surface (GlassWindow), so the compositor's
// lens bends the backdrop along every tile's edges. A click outside closes.
Scope {
    id: cc
    property var shell
    property bool open: false

    // Placement (from the top-right corner), as in the reference layout.
    readonly property int top: 38
    readonly property int edge: 10
    readonly property int gapS: 12
    readonly property int soundY: bright >= 0 ? 416 : 334
    readonly property int outY: soundY + 70 + gapS
    readonly property int editY: outY + (showOutputs ? outputs.implicitHeight + gapS : 0)
    readonly property int capY: editY + (editing ? edit.implicitHeight + gapS : 0)

    PanelWindow {
        visible: cc.open
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.namespace: "solyra-control-catcher"
        MouseArea { anchors.fill: parent; onClicked: cc.open = false }
    }

    property string ssid: ""
    property bool wifiOn: false
    property bool btOn: false
    property bool airplane: false
    property real bright: -1
    property bool showOutputs: false
    property bool editing: false
    readonly property var sink: Pipewire.defaultAudioSink
    readonly property var sinks: Pipewire.nodes.values.filter(n => n.isSink && n.audio && !n.isStream)
    readonly property var streams: Pipewire.nodes.values.filter(n => n.isStream && n.audio && !n.isSink)
    PwObjectTracker { objects: [Pipewire.defaultAudioSink].concat(cc.streams).concat(cc.sinks) }
    readonly property var player: {
        const ps = Mpris.players.values
        for (const p of ps) if (p.isPlaying) return p
        return ps.length ? ps[0] : null
    }
    function appName(n) { const p = n.properties || {}; return p["application.name"] || n.description || n.nickname || n.name || "App" }

    function refresh() { wifiProc.running = true; btProc.running = true; brightProc.running = true; rfProc.running = true }
    onOpenChanged: if (open) { refresh(); showOutputs = false; editing = false }
    Process { id: wifiProc; command: ["sh", "-c", "nmcli -t -f WIFI radio 2>/dev/null; nmcli -t -f ACTIVE,SSID dev wifi 2>/dev/null | grep '^yes' | cut -d: -f2"]
        stdout: StdioCollector { onStreamFinished: { const l = text.split("\n"); cc.wifiOn = l[0] === "enabled"; cc.ssid = l[1] || "" } } }
    Process { id: btProc; command: ["sh", "-c", "bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on"]
        stdout: StdioCollector { onStreamFinished: cc.btOn = text.trim() === "on" } }
    // Airplane Mode: every radio soft-blocked (no radios at all is not "on").
    Process { id: rfProc; command: ["sh", "-c", "out=$(rfkill list 2>/dev/null); [ -n \"$out\" ] && ! printf '%s' \"$out\" | grep -q 'Soft blocked: no' && echo blocked"]
        stdout: StdioCollector { onStreamFinished: cc.airplane = text.trim() === "blocked" } }
    Process { id: brightProc; command: ["sh", "-c", "brightnessctl -m 2>/dev/null | cut -d, -f4 | tr -d %"]
        stdout: StdioCollector { onStreamFinished: cc.bright = text.trim() ? parseInt(text) : -1 } }

    // ------------------------------------------------------------ pieces
    // A connectivity pill: disc + title + status, like macOS's.
    component Pill2: GlassWindow {
        id: p
        property string icon; property string title; property string sub; property bool on
        property color tint: Theme.accent
        signal clicked()
        visible: cc.open; fromRight: true
        r: implicitHeight / 2; kind: "clear"
        implicitWidth: 172; implicitHeight: 62
        Rectangle {
            id: disc
            x: 8; anchors.verticalCenter: parent.verticalCenter
            width: 46; height: 46; radius: 23
            color: p.on ? "white" : Qt.rgba(1, 1, 1, 0.16)
            Icon { anchors.centerIn: parent; name: p.icon; size: 20; color: p.on ? p.tint : "white" }
        }
        Column {
            anchors { left: disc.right; leftMargin: 10; right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
            Text { width: parent.width; elide: Text.ElideRight; text: p.title; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
            Text { width: parent.width; elide: Text.ElideRight; text: p.sub; color: Qt.rgba(1, 1, 1, 0.75); font.family: Theme.font; font.pixelSize: 13 }
        }
        TapHandler { onTapped: p.clicked() }
    }
    // A round button.
    component Round: GlassWindow {
        id: rb
        property string icon; property bool on: false
        signal clicked()
        visible: cc.open; fromRight: true
        r: implicitHeight / 2; kind: "clear"
        implicitWidth: 62; implicitHeight: 62
        Rectangle { anchors.fill: parent; anchors.margins: 1; radius: height / 2; color: "white"; visible: rb.on }
        Icon { anchors.centerIn: parent; name: rb.icon; size: 24; color: rb.on ? "#1c1c1e" : "white" }
        TapHandler { onTapped: rb.clicked() }
    }
    // A slider module: title + Cupertino/macOS 27 thick glass-pill slider with
    // low/high symbols and optional output button.
    component Track: GlassWindow {
        id: tr
        property string title; property string lowIcon; property string highIcon
        property real value: 0
        property bool extra: false
        signal moved(real v)
        signal extraClicked()
        fromRight: true
        r: 24; kind: "clear"
        implicitWidth: 348; implicitHeight: 70
        Text { x: 18; y: 10; text: tr.title; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Bold }
        Icon { id: lowI; x: 16; y: 39; name: tr.lowIcon; size: 15 }
        Item {
            id: track
            anchors { left: lowI.right; leftMargin: 10; right: highI.left; rightMargin: 10 }
            y: 36; height: 22
            Rectangle {
                anchors.fill: parent; radius: 11
                color: Qt.rgba(1, 1, 1, 0.16)
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.18)
                clip: true
                Rectangle {
                    height: parent.height; radius: 11
                    width: Math.max(22, parent.width * Math.max(0, Math.min(1, tr.value)))
                    color: "white"
                }
            }
            MouseArea {
                anchors.fill: parent; anchors.margins: -6
                function set(x) { tr.moved(Math.max(0, Math.min(1, (x - 6) / track.width))) }
                onPressed: ev => set(ev.x)
                onPositionChanged: ev => { if (pressed) set(ev.x) }
            }
        }
        Icon { id: highI; anchors.right: extraBtn.visible ? extraBtn.left : parent.right; anchors.rightMargin: extraBtn.visible ? 10 : 16; y: 38; name: tr.highIcon; size: 18 }
        Rectangle {
            id: extraBtn
            visible: tr.extra
            anchors.right: parent.right; anchors.rightMargin: 12; y: 34
            width: 26; height: 26; radius: 13; color: Qt.rgba(1, 1, 1, 0.16)
            Icon { anchors.centerIn: parent; name: "airplay"; size: 14 }
            TapHandler { onTapped: tr.extraClicked() }
        }
    }

    // ------------------------------------------------------------ layout
    Pill2 { gx: 186; gy: cc.top; icon: "wifi"; title: "Wi-Fi"; sub: cc.wifiOn ? (cc.ssid || "Not Connected") : "Off"; on: cc.wifiOn
        onClicked: { Quickshell.execDetached(["nmcli", "radio", "wifi", cc.wifiOn ? "off" : "on"]); cc.wifiOn = !cc.wifiOn } }
    Pill2 { gx: 186; gy: cc.top + 74; icon: "bluetooth"; title: "Bluetooth"; sub: cc.btOn ? "On" : "Off"; on: cc.btOn
        onClicked: { Quickshell.execDetached(["bluetoothctl", "power", cc.btOn ? "off" : "on"]); cc.btOn = !cc.btOn } }
    Pill2 { gx: 186; gy: cc.top + 148; icon: "airplane"; title: "Airplane Mode"; sub: cc.airplane ? "On" : "Off"; on: cc.airplane; tint: "#ff9f0a"
        onClicked: { Quickshell.execDetached(["rfkill", cc.airplane ? "unblock" : "block", "all"]); cc.airplane = !cc.airplane } }

    // Now Playing.
    GlassWindow {
        visible: cc.open; fromRight: true
        gx: cc.edge; gy: cc.top
        r: 26; kind: "clear"
        implicitWidth: 164; implicitHeight: 136
        Rectangle {
            x: 14; y: 14; width: 46; height: 46; radius: 10; color: Qt.rgba(1, 1, 1, 0.14); clip: true
            Image { id: artImg; anchors.fill: parent; source: cc.player ? cc.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
            Icon { visible: artImg.status !== Image.Ready; anchors.centerIn: parent; name: "music"; size: 20 }
        }
        Column {
            x: 14; y: 66; width: 136
            Text { width: parent.width; elide: Text.ElideRight; text: cc.player ? (cc.player.trackTitle || "Unknown") : "Not Playing"; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Bold }
            Text { width: parent.width; elide: Text.ElideRight; text: cc.player ? [cc.player.trackArtist, cc.player.trackAlbum].filter(x => x).join(" – ") : ""; color: Qt.rgba(1, 1, 1, 0.7); font.family: Theme.font; font.pixelSize: 12 }
        }
        Row {
            x: 28; y: 104; spacing: 26
            Icon { name: "prev"; size: 18; TapHandler { onTapped: if (cc.player) cc.player.previous() } }
            Icon { name: cc.player && cc.player.isPlaying ? "pause" : "play"; size: 20; TapHandler { onTapped: if (cc.player) cc.player.togglePlaying() } }
            Icon { name: "next"; size: 18; TapHandler { onTapped: if (cc.player) cc.player.next() } }
        }
    }
    Round { gx: 98; gy: cc.top + 148; implicitWidth: 76; icon: "snaplayout"; onClicked: { cc.open = false; cc.shell.snap() } }
    Round { gx: cc.edge; gy: cc.top + 148; implicitWidth: 76; icon: "record"; on: cc.shell && cc.shell.recording; onClicked: { cc.open = false; cc.shell.toggleRecording() } }

    Round { gx: 296; gy: cc.top + 222; icon: "halfcircle"; on: !Theme.dark; onClicked: { Theme.dark = !Theme.dark; Theme.save() } }
    GlassWindow {
        visible: cc.open; fromRight: true
        gx: 124; gy: cc.top + 222
        r: 31; kind: "clear"
        implicitWidth: 160; implicitHeight: 62
        Rectangle {
            id: moon
            x: 8; anchors.verticalCenter: parent.verticalCenter
            width: 46; height: 46; radius: 23
            color: cc.shell && cc.shell.dnd ? "white" : Qt.rgba(1, 1, 1, 0.16)
            Icon { anchors.centerIn: parent; name: "moon"; size: 20; color: cc.shell && cc.shell.dnd ? "#5e5ce6" : "white" }
        }
        Text { anchors.left: moon.right; anchors.leftMargin: 12; anchors.verticalCenter: parent.verticalCenter; text: "Focus"; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
        TapHandler { onTapped: cc.shell.dnd = !cc.shell.dnd }
    }

    Track {
        visible: cc.open && cc.bright >= 0
        gx: cc.edge; gy: cc.top + 296
        title: "Display"; lowIcon: "sunsmall"; highIcon: "sunlarge"; value: cc.bright / 100
        onMoved: v => { cc.bright = v * 100; Quickshell.execDetached(["brightnessctl", "-q", "set", Math.max(1, Math.round(v * 100)) + "%"]) }
    }
    Track {
        visible: cc.open
        gx: cc.edge; gy: cc.soundY
        title: "Sound"; lowIcon: "speakerlow"; highIcon: "speakerhigh"; extra: true
        value: cc.sink && cc.sink.audio ? cc.sink.audio.volume : 0
        onMoved: v => { if (cc.sink && cc.sink.audio) { cc.sink.audio.muted = false; cc.sink.audio.volume = v } }
        onExtraClicked: cc.showOutputs = !cc.showOutputs
    }

    // Outputs and per-app volume (the Sound module's output button).
    GlassWindow {
        id: outputs
        visible: cc.open && cc.showOutputs; fromRight: true
        gx: cc.edge; gy: cc.outY
        r: 22; kind: "clear"
        implicitWidth: 348; implicitHeight: outCol.height + 24
        Column {
            id: outCol
            x: 14; y: 12; width: 320; spacing: 6
            Text { text: "Output"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
            Repeater {
                model: cc.sinks
                Item {
                    required property var modelData
                    width: outCol.width; height: 30
                    Rectangle { width: 22; height: 22; radius: 11; anchors.verticalCenter: parent.verticalCenter
                        color: cc.sink === modelData ? Theme.accent : Qt.rgba(1, 1, 1, 0.14)
                        Icon { anchors.centerIn: parent; name: "speakerhigh"; size: 12 } }
                    Text { x: 32; width: parent.width - 32; anchors.verticalCenter: parent.verticalCenter; elide: Text.ElideRight; text: modelData.description || modelData.name; color: "white"; font.family: Theme.font; font.pixelSize: 13 }
                    TapHandler { onTapped: Pipewire.preferredDefaultAudioSink = modelData }
                }
            }
            Text { visible: cc.streams.length > 0; topPadding: 6; text: "Apps"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
            Repeater {
                model: cc.streams
                Item {
                    required property var modelData
                    width: outCol.width; height: 34
                    Text { width: 100; anchors.verticalCenter: parent.verticalCenter; elide: Text.ElideRight; text: cc.appName(modelData); color: "white"; font.family: Theme.font; font.pixelSize: 13 }
                    Item {
                        id: at
                        x: 110; width: parent.width - 110; height: parent.height
                        Rectangle { anchors.verticalCenter: parent.verticalCenter; width: parent.width; height: 4; radius: 2; color: Qt.rgba(1, 1, 1, 0.22) }
                        Rectangle { anchors.verticalCenter: parent.verticalCenter; width: Math.max(4, parent.width * Math.min(1, modelData.audio ? modelData.audio.volume : 0)); height: 4; radius: 2; color: "white" }
                        MouseArea {
                            anchors.fill: parent
                            function set(x) { if (modelData.audio) modelData.audio.volume = Math.max(0, Math.min(1, x / at.width)) }
                            onPressed: ev => set(ev.x)
                            onPositionChanged: ev => { if (pressed) set(ev.x) }
                        }
                    }
                }
            }
        }
    }

    // Edit Controls: everything else.
    GlassWindow {
        id: edit
        visible: cc.open && cc.editing; fromRight: true
        gx: cc.edge; gy: cc.editY
        r: 22; kind: "clear"
        implicitWidth: 348; implicitHeight: editCol.height + 24
        Column {
            id: editCol
            x: 14; y: 12; width: 320; spacing: 10
            Flow {
                width: parent.width; spacing: 8
                Pill { height: 28; text: "Night Shift"; on: Theme.nightShift; onClicked: { Theme.nightShift = !Theme.nightShift; Theme.save() } }
                Pill { height: 28; text: "Auto-arrange"; on: Theme.autoArrange; onClicked: cc.shell.setArrange(!Theme.autoArrange) }
                Pill { height: 28; text: "Reduce Motion"; on: Theme.reduceMotion; onClicked: { Theme.reduceMotion = !Theme.reduceMotion; Theme.save() } }
                Pill { height: 28; text: "Reduce Transparency"; on: Theme.reduceTransparency; onClicked: { Theme.reduceTransparency = !Theme.reduceTransparency; Theme.save() } }
                Pill { height: 28; text: "Borders"; on: Theme.showBorders; onClicked: { Theme.showBorders = !Theme.showBorders; Theme.save() } }
                Pill { height: 28; text: "Hide Notch"; on: Theme.hideNotch; onClicked: { Theme.hideNotch = !Theme.hideNotch; Theme.save() } }
                Pill { height: 28; text: "Hot Corners"; on: Theme.hotCorners; onClicked: { Theme.hotCorners = !Theme.hotCorners; Theme.save() } }
                Pill { height: 28; text: "Widgets"; on: Theme.widgets; onClicked: { Theme.widgets = !Theme.widgets; Theme.save() } }
                Pill { height: 28; text: "Dock Magnify"; on: Theme.dockMagnify; onClicked: { Theme.dockMagnify = !Theme.dockMagnify; Theme.save() } }
                Pill { height: 28; text: "Dock Autohide"; on: Theme.dockAutohide; onClicked: { Theme.dockAutohide = !Theme.dockAutohide; Theme.save() } }
            }
            Text { text: "Glass  ·  clear → tinted"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
            Item {
                id: gt
                width: parent.width; height: 16
                Rectangle { anchors.verticalCenter: parent.verticalCenter; width: parent.width; height: 4; radius: 2; color: Qt.rgba(1, 1, 1, 0.22) }
                Rectangle { anchors.verticalCenter: parent.verticalCenter; width: Math.max(4, parent.width * Theme.glass / 100); height: 4; radius: 2; color: "white" }
                MouseArea {
                    anchors.fill: parent; anchors.margins: -6
                    function set(x) { Theme.glass = Math.round(Math.max(0, Math.min(1, (x - 6) / gt.width)) * 100); Theme.save() }
                    onPressed: ev => set(ev.x)
                    onPositionChanged: ev => { if (pressed) set(ev.x) }
                }
            }
        }
    }

    // "Edit Controls".
    GlassWindow {
        visible: cc.open; fromRight: true
        gx: cc.edge + (348 - implicitWidth) / 2; gy: cc.capY
        r: 15; kind: "clear"
        implicitWidth: editLabel.implicitWidth + 30; implicitHeight: 30
        Text { id: editLabel; anchors.centerIn: parent; text: cc.editing ? "Done" : "Edit Controls"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
        TapHandler { onTapped: cc.editing = !cc.editing }
    }
}
