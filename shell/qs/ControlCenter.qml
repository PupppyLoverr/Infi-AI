import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire

Scope {
    id: cc
    property var shell
    property bool open: false
    property bool displayed: false
    property bool editing: false
    property string detail: ""
    property string lastDetail: ""
    property string ssid: ""
    property bool wifiOn: false
    property bool btOn: false
    property bool airplane: false
    property bool micOn: false
    property real bright: -1
    property real keyboardBright: -1
    property real pendingVolume: 0
    property var wifiNetworks: []
    property var bluetoothDevices: []
    property var order: ["wifi", "bluetooth", "nowPlaying", "microphone", "dock", "lock", "mute", "keyboard", "focus", "display", "sound", "airplane", "snap", "recording", "darkMode", "wallpaper"]
    property var visibleControls: ["wifi", "bluetooth", "nowPlaying", "microphone", "dock", "lock", "mute", "keyboard", "focus", "display", "sound"]
    property var editOrder: []
    property var editVisible: []
    readonly property string home: Quickshell.env("HOME")
    readonly property string activeDetail: detail || lastDetail
    readonly property string configPath: home + "/.config/solyra/controlcenter.json"
    readonly property var extras: ["airplane", "snap", "recording", "darkMode", "wallpaper"]
    readonly property var capsuleSlots: [
        { id: "wifi", x: 8, y: 8 },
        { id: "bluetooth", x: 8, y: 84 },
        { id: "microphone", x: 8, y: 160 },
        { id: "focus", x: 160, y: 236 }
    ]
    readonly property var circleSlots: [
        { id: "dock", x: 160, y: 160 },
        { id: "lock", x: 236, y: 160 },
        { id: "mute", x: 8, y: 236 },
        { id: "keyboard", x: 84, y: 236 }
    ]
    readonly property var sink: Pipewire.defaultAudioSink
    readonly property var sinks: Pipewire.nodes.values.filter(n => n.isSink && n.audio && !n.isStream)
    readonly property var streams: Pipewire.nodes.values.filter(n => n.isStream && n.audio && !n.isSink)
    readonly property var player: {
        const players = Mpris.players.values
        for (const p of players) if (p.isPlaying) return p
        return players.length ? players[0] : null
    }
    readonly property var soundRows: {
        const rows = []
        for (const n of sinks) rows.push({ kind: "sink", node: n })
        for (const n of streams) rows.push({ kind: "stream", node: n })
        return rows.slice(0, 9)
    }
    PwObjectTracker { objects: [Pipewire.defaultAudioSink].concat(cc.streams).concat(cc.sinks) }

    function hasControl(name) { return visibleControls.indexOf(name) >= 0 }
    function isCapsuleExtra(name) { return name === "airplane" || name === "wallpaper" }
    function isCircleExtra(name) { return name === "snap" || name === "recording" || name === "darkMode" }
    function extraPosition(name) {
        const slots = isCapsuleExtra(name) ? capsuleSlots : circleSlots
        const free = slots.filter(slot => !hasControl(slot.id))
        const candidates = order.filter(id => extras.indexOf(id) >= 0
            && (isCapsuleExtra(id) === isCapsuleExtra(name))
            && hasControl(id))
        const index = candidates.indexOf(name)
        return index >= 0 && index < free.length ? free[index] : { x: -1000, y: -1000 }
    }
    function canShow(name) {
        if (extras.indexOf(name) < 0) return true
        const slots = isCapsuleExtra(name) ? capsuleSlots : circleSlots
        const freeCount = slots.filter(slot => !hasControl(slot.id)).length
        const activeCount = order.filter(id => extras.indexOf(id) >= 0
            && (isCapsuleExtra(id) === isCapsuleExtra(name))
            && hasControl(id)).length
        return freeCount > activeCount
    }
    function setControl(name, on) {
        if (on && !hasControl(name) && !canShow(name)) return
        let next = visibleControls.slice()
        if (on && next.indexOf(name) < 0) next.push(name)
        if (!on) next = next.filter(id => id !== name)
        visibleControls = next
        if (order.indexOf(name) < 0) order = order.concat([name])
    }
    function startEdit() {
        open = true
        detail = ""
        editOrder = order.slice()
        editVisible = visibleControls.slice()
        editing = true
    }
    function cancelEdit() {
        if (editOrder.length) order = editOrder.slice()
        if (editVisible.length) visibleControls = editVisible.slice()
        editing = false
    }
    function finishEdit() {
        editing = false
        controlFile.setText(JSON.stringify({ order: order, visible: visibleControls }, null, 2))
    }
    function showDetail(name) {
        if (name === "" || ["wifi", "bluetooth", "display", "sound"].indexOf(name) >= 0) {
            detail = name
            editing = false
            if (name !== "") refresh()
        }
    }
    function appName(node) {
        const p = node.properties || {}
        return p["application.name"] || node.description || node.nickname || node.name || "App"
    }
    function refresh() {
        wifiStatus.running = true
        btStatus.running = true
        airplaneStatus.running = true
        brightnessStatus.running = true
        keyboardBrightnessStatus.running = true
        microphoneStatus.running = true
        if (detail === "wifi") wifiScan.running = true
        if (detail === "bluetooth") bluetoothScan.running = true
    }
    function toggleWifi() {
        wifiOn = !wifiOn
        Quickshell.execDetached(["nmcli", "radio", "wifi", wifiOn ? "on" : "off"])
    }
    function toggleBluetooth() {
        btOn = !btOn
        Quickshell.execDetached(["bluetoothctl", "power", btOn ? "on" : "off"])
    }
    function toggleAirplane() {
        airplane = !airplane
        Quickshell.execDetached(["rfkill", airplane ? "block" : "unblock", "all"])
    }
    function toggleMic() {
        micOn = !micOn
        Quickshell.execDetached(["wpctl", "set-mute", "@DEFAULT_AUDIO_SOURCE@", micOn ? "0" : "1"])
    }
    function toggleMute() {
        if (sink && sink.audio) sink.audio.muted = !sink.audio.muted
        else Quickshell.execDetached(["wpctl", "set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
    }
    function setDisplay(value) {
        if (bright < 0) return
        const level = Math.round(Math.max(0, Math.min(1, value)) * 100)
        bright = level
        brightnessWrite.command = ["brightnessctl", "-q", "set", level + "%"]
        brightnessWrite.running = true
    }
    function setKeyboardBrightness(value) {
        if (keyboardBright < 0) return
        const level = Math.round(Math.max(0, Math.min(1, value)) * 100)
        keyboardBright = level
        keyboardBrightnessWrite.command = ["brightnessctl", "-d", "*::kbd_backlight", "-q", "set", level + "%"]
        keyboardBrightnessWrite.running = true
    }
    function setVolume(value) {
        pendingVolume = Math.max(0, Math.min(1, value))
        volumeWrite.restart()
    }
    function connectWifi(network) {
        if (!network.ssid) return
        Quickshell.execDetached([
            "sh", "-c",
            "nmcli connection up id \"$1\" 2>/dev/null || nmcli device wifi connect \"$1\"",
            "sh", network.ssid
        ])
        ssid = network.ssid
        wifiOn = true
    }
    function connectBluetooth(device) {
        Quickshell.execDetached(["bluetoothctl", "connect", device.address])
    }
    function activateExtra(name) {
        if (name === "airplane") toggleAirplane()
        else if (name === "snap") { open = false; shell.snap() }
        else if (name === "recording") { open = false; shell.toggleRecording() }
        else if (name === "darkMode") { Theme.dark = !Theme.dark; Theme.save() }
        else if (name === "wallpaper") { open = false; Quickshell.execDetached(["solyra-settings"]) }
    }
    onOpenChanged: {
        if (open) {
            displayed = true
            detail = ""
            editing = false
            refresh()
        } else {
            closeAnimation.restart()
        }
    }
    onDetailChanged: {
        if (detail === "") {
            if (lastDetail !== "") detailExit.restart()
        } else {
            lastDetail = detail
            detailExit.stop()
            if (detail === "wifi") wifiScan.running = true
            if (detail === "bluetooth") bluetoothScan.running = true
        }
    }

    FileView {
        id: controlFile
        path: cc.configPath
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                const saved = JSON.parse(text())
                const valid = ["wifi", "bluetooth", "nowPlaying", "microphone", "dock", "lock", "mute", "keyboard", "focus", "display", "sound", "airplane", "snap", "recording", "darkMode", "wallpaper"]
                if (Array.isArray(saved.order)) cc.order = saved.order.filter(id => valid.indexOf(id) >= 0)
                    .concat(valid.filter(id => saved.order.indexOf(id) < 0))
                if (Array.isArray(saved.visible)) cc.visibleControls = saved.visible.filter(id => valid.indexOf(id) >= 0)
            } catch (e) {}
        }
    }
    Timer {
        id: closeAnimation
        interval: Theme.reduceMotion ? 0 : 180
        onTriggered: if (!cc.open) cc.displayed = false
    }
    Timer {
        id: detailExit
        interval: Theme.ms(350)
        onTriggered: if (cc.detail === "") cc.lastDetail = ""
    }
    Timer {
        interval: 1000
        repeat: true
        running: cc.open
        onTriggered: cc.refresh()
    }
    Timer {
        id: volumeWrite
        interval: 16
        repeat: false
        onTriggered: {
            if (cc.sink && cc.sink.audio) {
                cc.sink.audio.muted = false
                cc.sink.audio.volume = cc.pendingVolume
            }
        }
    }

    Process {
        id: wifiStatus
        command: ["sh", "-c", "nmcli -t -f WIFI radio 2>/dev/null; nmcli -t -f ACTIVE,SSID dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1"]
        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.split("\n")
                cc.wifiOn = lines[0] === "enabled"
                cc.ssid = lines[1] || ""
            }
        }
    }
    Process {
        id: btStatus
        command: ["sh", "-c", "bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on"]
        stdout: StdioCollector { onStreamFinished: cc.btOn = text.trim() === "on" }
    }
    Process {
        id: airplaneStatus
        command: ["sh", "-c", "out=$(rfkill list 2>/dev/null); [ -n \"$out\" ] && ! printf '%s' \"$out\" | grep -q 'Soft blocked: no' && echo blocked"]
        stdout: StdioCollector { onStreamFinished: cc.airplane = text.trim() === "blocked" }
    }
    Process {
        id: brightnessStatus
        command: ["sh", "-c", "brightnessctl -m 2>/dev/null | cut -d, -f4 | tr -d %"]
        stdout: StdioCollector { onStreamFinished: cc.bright = text.trim() ? parseInt(text) : -1 }
    }
    Process {
        id: keyboardBrightnessStatus
        command: ["sh", "-c", "brightnessctl -d '*::kbd_backlight' -m 2>/dev/null | head -1 | cut -d, -f4 | tr -d %"]
        stdout: StdioCollector { onStreamFinished: cc.keyboardBright = text.trim() ? parseInt(text) : -1 }
    }
    Process {
        id: microphoneStatus
        command: ["sh", "-c", "wpctl get-volume @DEFAULT_AUDIO_SOURCE@ 2>/dev/null"]
        stdout: StdioCollector { onStreamFinished: cc.micOn = text.trim() !== "" && text.indexOf("[MUTED]") < 0 }
    }
    Process { id: brightnessWrite }
    Process { id: keyboardBrightnessWrite }
    Process {
        id: wifiScan
        command: ["nmcli", "-t", "--escape", "no", "-f", "IN-USE,SSID,SECURITY,SIGNAL", "device", "wifi", "list", "--rescan", "no"]
        stdout: StdioCollector {
            onStreamFinished: {
                const rows = []
                for (const line of text.split("\n")) {
                    const fields = line.split(":")
                    if (fields.length < 4 || !fields[1]) continue
                    rows.push({ active: fields[0] === "*", ssid: fields[1], security: fields[2], signal: fields[3] })
                }
                cc.wifiNetworks = rows
            }
        }
    }
    Process {
        id: bluetoothScan
        command: ["bluetoothctl", "devices"]
        stdout: StdioCollector {
            onStreamFinished: {
                const rows = []
                for (const line of text.split("\n")) {
                    const match = line.match(/^Device\s+(\S+)\s+(.+)$/)
                    if (match) rows.push({ address: match[1], name: match[2] })
                }
                cc.bluetoothDevices = rows
            }
        }
    }

    component TileFrame: Item {
        id: tile
        property string controlId
        property real cornerRadius: 20
        property bool tappable: true
        signal activated()
        default property alias content: body.data
        opacity: cc.editing ? 0.55 : 1
        visible: cc.displayed && cc.detail === "" && cc.hasControl(controlId)
            && (cc.extras.indexOf(controlId) < 0 || cc.extraPosition(controlId).x >= 0)
        Behavior on opacity { NumberAnimation { duration: Theme.ms(150) } }
        Glass {
            anchors.fill: parent
            kind: "module"
            r: tile.cornerRadius
            shadow: false
        }
        Item { id: body; anchors.fill: parent }
        TapHandler {
            enabled: !cc.editing && tile.tappable
            onTapped: tile.activated()
        }
        MouseArea {
            visible: cc.editing
            anchors.fill: parent
            onClicked: cc.setControl(tile.controlId, false)
        }
        Rectangle {
            visible: cc.editing
            x: tile.width - 9; y: -9
            width: 18; height: 18; radius: 9
            color: "#ff453a"
            Text {
                anchors.centerIn: parent
                text: "−"; color: "white"
                font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wBold
            }
        }
    }
    component CapsuleFace: Item {
        property string icon
        property string title
        property string state
        property bool active: false
        property color tint: "#1c1c1e"
        Rectangle {
            x: 14; y: 14; width: 36; height: 36; radius: 18
            color: parent.active ? "white" : Qt.rgba(1, 1, 1, 0.11)
            Icon {
                anchors.centerIn: parent
                name: parent.parent.icon
                size: 17
                color: parent.parent.active ? parent.parent.tint : "white"
            }
        }
        Column {
            x: 56; anchors.verticalCenter: parent.verticalCenter
            width: parent.width - 64; spacing: 1
            Text {
                width: parent.width; elide: Text.ElideRight
                text: parent.parent.title; color: "white"
                font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
            }
            Text {
                width: parent.width; elide: Text.ElideRight
                text: parent.parent.state; color: Qt.rgba(1, 1, 1, 0.72)
                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
            }
        }
    }
    component RoundFace: Item {
        property string icon
        property bool active: false
        Rectangle {
            anchors.fill: parent; anchors.margins: 1
            radius: width / 2; color: "white"; visible: parent.active
        }
        Icon {
            anchors.centerIn: parent
            name: parent.icon; size: 22
            color: parent.active ? "#1c1c1e" : "white"
        }
    }
    component LevelBar: FocusScope {
        id: level
        property real value: 0
        property int trackX: 28
        property int trackWidth: 218
        property bool outputButton: false
        signal moved(real value)
        signal outputClicked()
        signal detailsClicked()
        width: 292; height: 64
        function setAt(x) {
            moved(Math.max(0, Math.min(1, (x - trackX) / trackWidth)))
        }
        Text {
            x: 14; y: 9
            text: level.objectName
            color: "white"
            font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
            TapHandler { onTapped: level.detailsClicked() }
        }
        Rectangle {
            x: level.trackX; y: 40; width: level.trackWidth; height: 4; radius: 2
            color: Qt.rgba(1, 1, 1, 0.22)
            Rectangle {
                width: parent.width * Math.max(0, Math.min(1, level.value))
                height: parent.height; radius: 2
                color: "white"
            }
        }
        MouseArea {
            x: level.trackX - 12; y: 31
            width: level.trackWidth + 24; height: 22
            acceptedButtons: Qt.LeftButton
            onPressed: event => { level.forceActiveFocus(); level.setAt(event.x + x) }
            onPositionChanged: event => { if (pressed) level.setAt(event.x + x) }
            onWheel: event => {
                level.moved(Math.max(0, Math.min(1, level.value + (event.angleDelta.y > 0 ? 1 / 16 : -1 / 16))))
                event.accepted = true
            }
        }
        Rectangle {
            visible: level.outputButton
            x: 244; y: 29; width: 26; height: 26; radius: 13
            color: Qt.rgba(1, 1, 1, 0.18)
            Icon { anchors.centerIn: parent; name: "airplay"; size: 14 }
            TapHandler { onTapped: level.outputClicked() }
        }
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Left || event.key === Qt.Key_Down) {
                level.moved(Math.max(0, level.value - 1 / 16))
                event.accepted = true
            } else if (event.key === Qt.Key_Right || event.key === Qt.Key_Up) {
                level.moved(Math.min(1, level.value + 1 / 16))
                event.accepted = true
            }
        }
    }
    component DetailLevel: Item {
        id: dl
        property string label
        property real value
        signal moved(real value)
        width: 260; height: 32
        Text {
            anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
            width: 94; elide: Text.ElideRight
            text: dl.label; color: "white"
            font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
        }
        Rectangle {
            x: 100; y: 14; width: 150; height: 4; radius: 2
            color: Qt.rgba(1, 1, 1, 0.22)
            Rectangle { width: parent.width * Math.max(0, Math.min(1, dl.value)); height: 4; radius: 2; color: "white" }
        }
        MouseArea {
            x: 94; y: 5; width: 166; height: 22
            function setValue(position) { dl.moved(Math.max(0, Math.min(1, (position - 6) / 150))) }
            onPressed: event => setValue(event.x)
            onPositionChanged: event => { if (pressed) setValue(event.x) }
            onWheel: event => {
                dl.moved(Math.max(0, Math.min(1, dl.value + (event.angleDelta.y > 0 ? 1 / 16 : -1 / 16))))
                event.accepted = true
            }
        }
    }

    PanelWindow {
        id: catcher
        visible: cc.displayed
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.namespace: "solyra-control-catcher"
        MouseArea { anchors.fill: parent; onClicked: cc.open = false }
    }

    PanelWindow {
        id: panel
        visible: cc.displayed
        anchors { top: true; right: true }
        margins { top: 40; right: 6 }
        implicitWidth: 308
        implicitHeight: cc.editing ? 572 : 500
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        opacity: cc.open ? 1 : 0
        Behavior on opacity {
            NumberAnimation { duration: Theme.reduceMotion ? 0 : (cc.open ? 250 : 180) }
        }
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-controlcenter"
        WlrLayershell.keyboardFocus: cc.open ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

        TileFrame {
            controlId: "wifi"; x: 8; y: 8
            onActivated: cc.showDetail("wifi")
            CapsuleFace {
                anchors.fill: parent; icon: "wifi"; title: "Wi-Fi"
                state: cc.wifiOn ? (cc.ssid || "Connected") : "Off"
                active: cc.wifiOn
            }
        }
        TileFrame {
            controlId: "bluetooth"; x: 8; y: 84
            onActivated: cc.showDetail("bluetooth")
            CapsuleFace {
                anchors.fill: parent; icon: "bluetooth"; title: "Bluetooth"
                state: cc.btOn ? "On" : "Off"; active: cc.btOn
            }
        }
        TileFrame {
            controlId: "microphone"; x: 8; y: 160
            onActivated: cc.toggleMic()
            CapsuleFace {
                anchors.fill: parent; icon: "mic"; title: "Microphone"
                state: cc.micOn ? "On" : "Off"; active: cc.micOn
            }
        }
        TileFrame {
            controlId: "nowPlaying"; x: 160; y: 8
            width: 140; height: 140; cornerRadius: 32; tappable: false
            Rectangle {
                x: 14; y: 14; width: 40; height: 40; radius: 9
                color: Qt.rgba(1, 1, 1, 0.14); clip: true
                Image {
                    id: artwork
                    anchors.fill: parent
                    source: cc.player ? cc.player.trackArtUrl : ""
                    fillMode: Image.PreserveAspectCrop
                }
                Icon { visible: artwork.status !== Image.Ready; anchors.centerIn: parent; name: "music"; size: 18 }
            }
            Text {
                x: 14; y: 62; width: 112; elide: Text.ElideRight
                text: cc.player ? (cc.player.trackTitle || "Unknown") : "Not Playing"
                color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
            }
            Text {
                x: 14; y: 80; width: 112; elide: Text.ElideRight
                text: cc.player ? (cc.player.trackArtist || "") : ""
                color: Qt.rgba(1, 1, 1, 0.7); font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
            }
            Icon {
                x: 20; y: 101; name: "prev"; size: 20
                TapHandler { onTapped: if (cc.player) cc.player.previous() }
            }
            Icon {
                x: 60; y: 101; name: cc.player && cc.player.isPlaying ? "pause" : "play"; size: 20
                TapHandler { onTapped: if (cc.player) cc.player.togglePlaying() }
            }
            Icon {
                x: 100; y: 101; name: "next"; size: 20
                TapHandler { onTapped: if (cc.player) cc.player.next() }
            }
        }
        TileFrame {
            controlId: "dock"; x: 160; y: 160; width: 64; height: 64; cornerRadius: 32
            onActivated: { Theme.dockAutohide = !Theme.dockAutohide; Theme.save() }
            RoundFace { anchors.fill: parent; icon: "down"; active: Theme.dockAutohide }
        }
        TileFrame {
            controlId: "lock"; x: 236; y: 160; width: 64; height: 64; cornerRadius: 32
            onActivated: { cc.open = false; cc.shell.lock() }
            RoundFace { anchors.fill: parent; icon: "lock"; active: false }
        }
        TileFrame {
            controlId: "mute"; x: 8; y: 236; width: 64; height: 64; cornerRadius: 32
            onActivated: cc.toggleMute()
            RoundFace { anchors.fill: parent; icon: "volumeMute"; active: cc.sink && cc.sink.audio ? cc.sink.audio.muted : false }
        }
        TileFrame {
            controlId: "keyboard"; x: 84; y: 236; width: 64; height: 64; cornerRadius: 32
            onActivated: cc.setKeyboardBrightness(cc.keyboardBright > 0 ? 0 : 0.5)
            RoundFace { anchors.fill: parent; icon: "apps"; active: cc.keyboardBright > 0 }
        }
        TileFrame {
            controlId: "focus"; x: 160; y: 236; width: 140; height: 64
            onActivated: cc.shell.dnd = !cc.shell.dnd
            CapsuleFace {
                anchors.fill: parent; icon: "moon"; title: "Focus"
                state: cc.shell && cc.shell.dnd ? "Do Not Disturb" : "Off"
                active: cc.shell && cc.shell.dnd
            }
        }
        TileFrame {
            controlId: "display"; x: 8; y: 312; width: 292; height: 64
            cornerRadius: 24; tappable: false
            LevelBar {
                objectName: "Display"; anchors.fill: parent; value: cc.bright < 0 ? 0 : cc.bright / 100
                onMoved: value => cc.setDisplay(value)
                onDetailsClicked: cc.showDetail("display")
            }
        }
        TileFrame {
            controlId: "sound"; x: 8; y: 388; width: 292; height: 64
            cornerRadius: 24; tappable: false
            LevelBar {
                objectName: "Sound"; anchors.fill: parent
                trackX: 25; trackWidth: 180; outputButton: true
                value: cc.sink && cc.sink.audio ? cc.sink.audio.volume : 0
                onMoved: value => cc.setVolume(value)
                onOutputClicked: cc.showDetail("sound")
            }
        }
        Repeater {
            model: cc.extras
            delegate: TileFrame {
                required property string modelData
                controlId: modelData
                x: cc.extraPosition(modelData).x
                y: cc.extraPosition(modelData).y
                width: cc.isCapsuleExtra(modelData) ? 140 : 64
                height: 64
                cornerRadius: cc.isCapsuleExtra(modelData) ? 20 : 32
                onActivated: cc.activateExtra(modelData)
                CapsuleFace {
                    anchors.fill: parent
                    visible: cc.isCapsuleExtra(modelData)
                    icon: modelData === "airplane" ? "airplane" : "image"
                    title: modelData === "airplane" ? "Airplane Mode" : "Wallpaper"
                    state: modelData === "airplane" ? (cc.airplane ? "On" : "Off") : "Choose Wallpaper"
                    active: modelData === "airplane" && cc.airplane
                }
                RoundFace {
                    anchors.fill: parent
                    visible: cc.isCircleExtra(modelData)
                    icon: modelData === "snap" ? "snaplayout" : modelData === "recording" ? "record" : "halfcircle"
                    active: modelData === "recording" ? (cc.shell && cc.shell.recording)
                        : modelData === "darkMode" ? Theme.dark : false
                }
            }
        }

        Item {
            id: detailView
            x: 8; y: 8; width: 292; height: 444
            visible: cc.displayed && cc.activeDetail !== ""
            opacity: cc.detail !== "" ? 1 : 0
            scale: cc.detail !== "" ? 1 : 0.96
            Behavior on opacity { NumberAnimation { duration: Theme.ms(350); easing.type: Easing.OutCubic } }
            Behavior on scale {
                enabled: !Theme.reduceMotion
                SpringAnimation { spring: 3.4; damping: 1.0; epsilon: 0.001 }
            }
            Glass { anchors.fill: parent; kind: "module"; r: 24; shadow: false }
            Rectangle {
                x: 14; y: 16; width: 32; height: 32; radius: 16
                color: Qt.rgba(1, 1, 1, 0.12)
                Icon { anchors.centerIn: parent; name: "left"; size: 15 }
                TapHandler { onTapped: cc.showDetail("") }
            }
            Text {
                x: 56; y: 12; width: 162; height: 48
                verticalAlignment: Text.AlignVCenter
                text: cc.activeDetail === "wifi" ? "Wi-Fi" : cc.activeDetail === "bluetooth" ? "Bluetooth"
                    : cc.activeDetail === "display" ? "Display" : "Sound"
                color: "white"; font.family: Theme.font; font.pixelSize: 28; font.weight: Theme.wTitle
            }
            Rectangle {
                visible: cc.activeDetail === "wifi" || cc.activeDetail === "bluetooth"
                x: 226; y: 18; width: 52; height: 28; radius: 14
                color: (cc.activeDetail === "wifi" ? cc.wifiOn : cc.btOn) ? Theme.accent : Qt.rgba(1, 1, 1, 0.14)
                Text {
                    anchors.centerIn: parent
                    text: (cc.activeDetail === "wifi" ? cc.wifiOn : cc.btOn) ? "On" : "Off"
                    color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wTitle
                }
                TapHandler { onTapped: cc.activeDetail === "wifi" ? cc.toggleWifi() : cc.toggleBluetooth() }
            }

            Flickable {
                visible: cc.activeDetail === "wifi"
                x: 14; y: 80; width: 264; height: 350
                clip: true; contentHeight: wifiRows.height; boundsBehavior: Flickable.StopAtBounds
                Column {
                    id: wifiRows
                    width: parent.width; spacing: 0
                    Repeater {
                        model: cc.wifiNetworks.slice(0, 9)
                        delegate: Item {
                            required property var modelData
                            width: wifiRows.width; height: 32
                            Text {
                                x: 4; width: 176; anchors.verticalCenter: parent.verticalCenter
                                elide: Text.ElideRight
                                text: modelData.ssid; color: "white"
                                font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                            }
                            Text {
                                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                                text: modelData.active ? "Connected" : modelData.security === "--" ? "Open" : modelData.signal + "%"
                                color: modelData.active ? "#30d158" : Qt.rgba(1, 1, 1, 0.65)
                                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
                            }
                            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.08) }
                            TapHandler { onTapped: cc.connectWifi(modelData) }
                        }
                    }
                    Text {
                        visible: cc.wifiNetworks.length === 0
                        width: parent.width; height: 32
                        text: "No networks found"; color: Qt.rgba(1, 1, 1, 0.62)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                    }
                }
            }
            Flickable {
                visible: cc.activeDetail === "bluetooth"
                x: 14; y: 80; width: 264; height: 350
                clip: true; contentHeight: bluetoothRows.height; boundsBehavior: Flickable.StopAtBounds
                Column {
                    id: bluetoothRows
                    width: parent.width; spacing: 0
                    Repeater {
                        model: cc.bluetoothDevices.slice(0, 9)
                        delegate: Item {
                            required property var modelData
                            width: bluetoothRows.width; height: 32
                            Text {
                                x: 4; width: 180; anchors.verticalCenter: parent.verticalCenter
                                elide: Text.ElideRight; text: modelData.name
                                color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                            }
                            Text {
                                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                                text: "Connect"; color: Qt.rgba(1, 1, 1, 0.65)
                                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
                            }
                            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.08) }
                            TapHandler { onTapped: cc.connectBluetooth(modelData) }
                        }
                    }
                    Text {
                        visible: cc.bluetoothDevices.length === 0
                        width: parent.width; height: 32
                        text: "No Bluetooth devices found"; color: Qt.rgba(1, 1, 1, 0.62)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                    }
                }
            }
            Column {
                visible: cc.activeDetail === "display"
                x: 16; y: 88; width: 260; spacing: 0
                DetailLevel {
                    label: "Display"; value: cc.bright < 0 ? 0 : cc.bright / 100
                    onMoved: value => cc.setDisplay(value)
                }
                DetailLevel {
                    label: "Keyboard backlight"; value: cc.keyboardBright < 0 ? 0 : cc.keyboardBright / 100
                    onMoved: value => cc.setKeyboardBrightness(value)
                }
                Text {
                    visible: cc.bright < 0 && cc.keyboardBright < 0
                    width: parent.width; height: 32
                    text: "Brightness controls unavailable"; color: Qt.rgba(1, 1, 1, 0.62)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                }
            }
            Flickable {
                visible: cc.activeDetail === "sound"
                x: 14; y: 80; width: 264; height: 350
                clip: true; contentHeight: soundRowsColumn.height; boundsBehavior: Flickable.StopAtBounds
                Column {
                    id: soundRowsColumn
                    width: parent.width; spacing: 0
                    Repeater {
                        model: cc.soundRows
                        delegate: Item {
                            required property var modelData
                            width: soundRowsColumn.width; height: 32
                            Text {
                                x: 4; width: modelData.kind === "sink" ? 205 : 92
                                anchors.verticalCenter: parent.verticalCenter
                                elide: Text.ElideRight
                                text: modelData.kind === "sink" ? (modelData.node.description || modelData.node.name) : cc.appName(modelData.node)
                                color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                            }
                            Rectangle {
                                visible: modelData.kind === "sink"
                                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                                width: 8; height: 8; radius: 4
                                color: cc.sink === modelData.node ? Theme.accent : Qt.rgba(1, 1, 1, 0.26)
                            }
                            Rectangle {
                                visible: modelData.kind === "stream"
                                x: 104; y: 14; width: 150; height: 4; radius: 2
                                color: Qt.rgba(1, 1, 1, 0.22)
                                Rectangle {
                                    width: parent.width * Math.max(0, Math.min(1, modelData.node.audio ? modelData.node.audio.volume : 0))
                                    height: 4; radius: 2; color: "white"
                                }
                                MouseArea {
                                    anchors.fill: parent; anchors.margins: -5
                                    function setVolumeAt(position) {
                                        if (modelData.node.audio) modelData.node.audio.volume = Math.max(0, Math.min(1, position / 150))
                                    }
                                    onPressed: event => setVolumeAt(event.x)
                                    onPositionChanged: event => { if (pressed) setVolumeAt(event.x) }
                                    onWheel: event => {
                                        if (modelData.node.audio) modelData.node.audio.volume = Math.max(0, Math.min(1,
                                            modelData.node.audio.volume + (event.angleDelta.y > 0 ? 1 / 16 : -1 / 16)))
                                        event.accepted = true
                                    }
                                }
                            }
                            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.08) }
                            TapHandler {
                                enabled: modelData.kind === "sink"
                                onTapped: {
                                    Pipewire.preferredDefaultAudioSink = modelData.node
                                    Quickshell.execDetached(["wpctl", "set-default", String(modelData.node.id)])
                                }
                            }
                        }
                    }
                    Text {
                        visible: cc.soundRows.length === 0
                        width: parent.width; height: 32
                        text: "No sound outputs"; color: Qt.rgba(1, 1, 1, 0.62)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                    }
                }
            }
        }

        Glass {
            visible: cc.displayed && cc.detail === "" && !cc.editing
            x: 107; y: 468; width: 94; height: 24; r: 12; kind: "clear"; shadow: false
            Text {
                anchors.centerIn: parent; text: "Edit Controls"; color: "white"
                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
            }
            TapHandler { onTapped: cc.startEdit() }
        }
        Flow {
            visible: cc.displayed && cc.editing
            x: 8; y: 460; width: 292; height: 72
            spacing: 8
            Repeater {
                model: cc.extras.filter(id => !cc.hasControl(id))
                delegate: Rectangle {
                    required property string modelData
                    width: editChip.implicitWidth + 22; height: 24; radius: 12
                    opacity: cc.canShow(modelData) ? 1 : 0.45
                    color: Qt.rgba(1, 1, 1, 0.10)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.14)
                    Text {
                        id: editChip; anchors.centerIn: parent
                        text: modelData === "airplane" ? "Airplane Mode"
                            : modelData === "snap" ? "Snap Layouts"
                            : modelData === "recording" ? "Screen Recording"
                            : modelData === "darkMode" ? "Dark Mode" : "Wallpaper"
                        color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
                    }
                    TapHandler { enabled: cc.canShow(modelData); onTapped: cc.setControl(modelData, true) }
                }
            }
        }
        Glass {
            visible: cc.displayed && cc.editing
            x: 94; y: 540; width: 62; height: 24; r: 12; kind: "clear"; shadow: false
            Text {
                anchors.centerIn: parent; text: "Cancel"; color: "white"
                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
            }
            TapHandler { onTapped: cc.cancelEdit() }
        }
        Glass {
            visible: cc.displayed && cc.editing
            x: 164; y: 540; width: 49; height: 24; r: 12; kind: "clear"; shadow: false
            Text {
                anchors.centerIn: parent; text: "Done"; color: "white"
                font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wTitle
            }
            TapHandler { onTapped: cc.finishEdit() }
        }
    }
}
