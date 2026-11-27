import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire
import Quickshell.Services.UPower
import Quickshell.Widgets

// The notch, after Droppy and Apple's Dynamic Island:
// - idle: a small black pill, centred, floating just below the top edge;
// - compact: live activity wings (album art + equaliser) while media plays;
// - HUD: volume / brightness / battery / caps lock, in the island;
// - expanded: pulls down from the top with a spring into tabs
//   (Media, Shelf, Clipboard, Tools: High Alert, Termi-Notch).
PanelWindow {
    id: win
    property var screenModel
    screen: screenModel
    anchors.top: true
    implicitWidth: 760
    implicitHeight: 320
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-island"
    WlrLayershell.keyboardFocus: termField.activeFocus ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
    mask: Region {
        Region { item: shape }
        Region { item: tabsPill }
    }

    // ------------------------------------------------------------ state
    property string mode: "idle"         // idle | compact | hud | expanded
    property string tab: "media"
    property string hudKind: ""
    property string hudText: ""
    property real hudValue: 0
    property bool hovering: false
    readonly property MprisPlayer player: {
        const ps = Mpris.players.values
        let best = null
        for (const p of ps) { if (p.isPlaying) return p; if (!best) best = p }
        return best
    }
    readonly property bool playing: player !== null && player.isPlaying

    function settle() {
        if (hovering || tab === "shelf" && dropping) { mode = "expanded"; return }
        if (hudTimer.running) { mode = "hud"; return }
        mode = playing ? "compact" : "idle"
    }
    function hud(kind, value, label) {
        hudKind = kind; hudValue = value; hudText = label || ""
        if (mode !== "expanded") { mode = "hud" }
        hudTimer.restart()
    }
    onPlayingChanged: if (mode !== "expanded" && mode !== "hud") settle()
    Timer { id: hudTimer; interval: 1600; onTriggered: win.settle() }
    Timer { id: openTimer; interval: 160; onTriggered: { win.hovering = true; win.mode = "expanded" } }
    Timer { id: closeTimer; interval: 380; onTriggered: { win.hovering = false; win.settle() } }
    Timer { interval: 1000; running: win.player !== null && win.playing; repeat: true; onTriggered: win.player.positionChanged() }

    // HUD sources: PipeWire volume, battery/charging, caps lock, brightness.
    PwObjectTracker { objects: [Pipewire.defaultAudioSink] }
    readonly property var sink: Pipewire.defaultAudioSink
    property real lastVol: -1
    Connections {
        target: win.sink ? win.sink.audio : null
        function onVolumeChanged() {
            const v = win.sink.audio.volume
            if (win.lastVol >= 0 && Math.abs(v - win.lastVol) > 0.001) win.hud(win.sink.audio.muted ? "mute" : "volume", v)
            win.lastVol = v
        }
        function onMutedChanged() { win.hud(win.sink.audio.muted ? "mute" : "volume", win.sink.audio.volume) }
    }
    readonly property var battery: UPower.displayDevice
    property int lastBatState: -1
    Connections {
        target: win.battery
        function onStateChanged() {
            const charging = win.battery.state === UPowerDeviceState.Charging
            if (win.lastBatState !== -1) win.hud(charging ? "charging" : "battery", win.battery.percentage)
            win.lastBatState = win.battery.state
        }
    }
    property string capsPath: ""
    Process {
        running: true
        command: ["sh", "-c", "ls -d /sys/class/leds/*capslock* 2>/dev/null | head -1"]
        stdout: StdioCollector { onStreamFinished: win.capsPath = text.trim() }
    }
    FileView {
        id: capsFile
        path: win.capsPath ? win.capsPath + "/brightness" : ""
        property int last: -1
        onLoaded: {
            const v = parseInt(text())
            if (last !== -1 && v !== last) win.hud(v ? "capsOn" : "capsOff", v)
            last = v
        }
    }
    Timer { interval: 400; running: win.capsPath !== ""; repeat: true; onTriggered: capsFile.reload() }

    // Bluetooth headphones: BlueZ "Connected" changes on the system bus.
    Process {
        running: true
        command: ["sh", "-c", "dbus-monitor --system \"type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.bluez.Device1'\" 2>/dev/null | awk '/path=/{p=$0} /Connected/{getline; if ($0 ~ /true/) {match(p, /path=[^;]*/); print substr(p, RSTART+5, RLENGTH-5); fflush()}}'"]
        stdout: SplitParser {
            onRead: path => { btName.command = ["sh", "-c", "bluetoothctl info $(echo \"$1\" | sed 's|.*dev_||; s|_|:|g') | sed -n 's/^\\s*Alias: //p'", "sh", path.trim()]; btName.running = true }
        }
    }
    Process { id: btName; stdout: StdioCollector { onStreamFinished: win.hud("bluetooth", 1, text.trim() || "Headphones") } }

    // Tracked Folders: new files in Downloads land on the shelf.
    Process {
        running: true
        command: ["sh", "-c", "command -v inotifywait >/dev/null || exit 0; inotifywait -m -q -e close_write -e moved_to --format '%w%f' \"$HOME/Downloads\""]
        stdout: SplitParser { onRead: f => { if (f && !/\.(part|crdownload|tmp)$/.test(f)) shelf.add("file://" + f) } }
    }

    // ------------------------------------------------------------ geometry
    readonly property var dims: ({
        idle:     { w: 126, h: 37, r: 18.5 },
        compact:  { w: 300, h: 37, r: 18.5 },
        hud:      { w: 330, h: 37, r: 18.5 },
        expanded: { w: 520, h: 214, r: 44 }
    })
    // Media and the calendar glance are shorter than the card tabs: fit them.
    readonly property var d: mode === "expanded" && (tab === "media" || tab === "cal")
        ? { w: 520, h: 172, r: 40 } : dims[mode]

    Rectangle {
        id: shape
        // Idle, compact and HUD: solid black, the camera housing. Expanded:
        // dark Liquid Glass over Hyprland's blur, with the lensing rim.
        readonly property bool glassy: win.mode === "expanded" && !Theme.reduceTransparency
        color: glassy ? Qt.rgba(0.03, 0.03, 0.04, 0.80) : "black"
        Behavior on color { ColorAnimation { duration: Theme.ms(220) } }
        opacity: Theme.hideNotch && win.mode === "idle" && !hoverIdle.hovered ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
        HoverHandler { id: hoverIdle }
        anchors.horizontalCenter: parent.horizontalCenter
        y: 5
        width: win.d.w
        height: win.d.h
        radius: win.d.r
        clip: true
        Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: win.mode === "expanded" ? Theme.springOpen : Theme.springClose; damping: win.mode === "expanded" ? Theme.dampOpen : Theme.dampClose; epsilon: 0.3 } }
        Behavior on height { enabled: !Theme.reduceMotion; SpringAnimation { spring: win.mode === "expanded" ? Theme.springOpen : Theme.springClose; damping: win.mode === "expanded" ? Theme.dampOpen : Theme.dampClose; epsilon: 0.3 } }
        Behavior on radius { NumberAnimation { duration: Theme.ms(260); easing.type: Easing.OutCubic } }

        HoverHandler {
            onHoveredChanged: {
                if (hovered) { closeTimer.stop(); openTimer.restart() }
                else { openTimer.stop(); closeTimer.restart() }
            }
        }
        TapHandler { onTapped: { if (win.mode === "expanded") { win.hovering = false; win.settle() } else { win.hovering = true; win.mode = "expanded" } } }
        DropArea {
            anchors.fill: parent
            onEntered: { win.tab = "shelf"; win.dropping = true; win.hovering = true; win.mode = "expanded" }
            onExited: win.dropping = false
            onDropped: drop => { win.dropping = false; for (const u of drop.urls) shelf.add(u.toString()) }
        }

        Shape {
            anchors.fill: parent
            visible: shape.glassy
            opacity: shape.glassy ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeWidth: 0
                fillRule: ShapePath.OddEvenFill
                fillGradient: LinearGradient {
                    x1: 0; y1: 0; x2: shape.width; y2: shape.height
                    GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.42) }
                    GradientStop { position: 0.4; color: Qt.rgba(1, 1, 1, 0.10) }
                    GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, 0.24) }
                }
                PathRectangle { x: 0; y: 0; width: shape.width; height: shape.height; radius: shape.radius }
                PathRectangle { x: 1.2; y: 1.2; width: shape.width - 2.4; height: shape.height - 2.4; radius: Math.max(0, shape.radius - 1.2) }
            }
        }
        Rectangle {
            anchors { left: parent.left; right: parent.right; top: parent.top }
            height: 50; radius: shape.radius
            visible: shape.glassy
            gradient: Gradient {
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, 0.10) }
                GradientStop { position: 1.0; color: "transparent" }
            }
        }

        // Compact live activity: art left, equaliser right.
        Item {
            anchors.fill: parent
            opacity: win.mode === "compact" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }
            ClippingRectangle {
                x: 9; anchors.verticalCenter: parent.verticalCenter
                width: 23; height: 23; radius: 6; color: "#333"
                Image { anchors.fill: parent; source: win.player ? win.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
            }
            Equaliser { anchors.right: parent.right; anchors.rightMargin: 13; anchors.verticalCenter: parent.verticalCenter; active: win.playing }
        }

        // HUD row.
        Row {
            anchors.centerIn: parent
            spacing: 12
            opacity: win.mode === "hud" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                size: 18
                name: ({ volume: "volume", mute: "volumeMute", brightness: "sun", battery: "battery", charging: "charging", capsOn: "up", capsOff: "up", bluetooth: "bluetooth", focus: "moon" })[win.hudKind] || "info"
                color: win.hudKind === "charging" ? "#30d158" : "white"
            }
            Text {
                visible: win.hudText !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: win.hudText + (win.hudKind === "bluetooth" ? "  · Connected" : "")
                color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            }
            Text {
                visible: win.hudKind.startsWith("caps")
                anchors.verticalCenter: parent.verticalCenter
                text: win.hudKind === "capsOn" ? "Caps Lock On" : "Caps Lock Off"
                color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            }
            Rectangle {
                visible: !win.hudKind.startsWith("caps") && win.hudText === ""
                anchors.verticalCenter: parent.verticalCenter
                width: 200; height: 6; radius: 3; color: Qt.rgba(1, 1, 1, 0.22)
                Rectangle {
                    width: parent.width * Math.max(0, Math.min(1, win.hudKind === "mute" ? 0 : (win.hudValue > 1.001 ? win.hudValue / 100 : win.hudValue)))
                    height: parent.height; radius: 3
                    color: win.hudKind === "charging" ? "#30d158" : "white"
                    Behavior on width { NumberAnimation { duration: Theme.ms(140) } }
                }
            }
        }

        // Expanded content: fades in after the shape leads (Apple staggers it).
        Item {
            id: content
            anchors.fill: parent
            anchors.margins: 18
            anchors.topMargin: 16
            opacity: win.mode === "expanded" && shape.height > 130 ? 1 : 0
            scale: opacity > 0 ? 1 : 0.96
            Behavior on opacity { NumberAnimation { duration: Theme.ms(200); easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: Theme.ms(240); easing.type: Easing.OutCubic } }

            MediaView { anchors.fill: parent; visible: win.tab === "media"; player: win.player }
            ShelfView { id: shelfView; anchors.fill: parent; visible: win.tab === "shelf"; model: shelf; dropping: win.dropping }
            ClipView { anchors.fill: parent; visible: win.tab === "clip"; history: clip }
            CalendarGlance { anchors.fill: parent; visible: win.tab === "cal" }
            Loader {
                anchors.fill: parent
                active: win.tab === "face"
                source: "NotchFace.qml"
                onLoaded: item.active = Qt.binding(() => win.tab === "face" && win.mode === "expanded")
                onStatusChanged: if (status === Loader.Error) faceError.visible = true
            }
            Text { id: faceError; visible: false; anchors.centerIn: parent; text: "Notchface needs QtMultimedia"; color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 13 }
            Item {
                anchors.fill: parent
                visible: win.tab === "tools"
                Column {
                    anchors.fill: parent
                    spacing: 12
                    Row {
                        spacing: 10
                        Pill {
                            text: caffeine.enabled ? "High Alert · " + Math.ceil(caffeineTimer.left / 60) + " min" : "High Alert"
                            on: caffeine.enabled
                            onClicked: caffeine.toggle()
                        }
                        Pill { text: "Lock"; onClicked: Quickshell.execDetached(["qs", "ipc", "call", "shell", "lock"]) }
                        Pill { text: "Screenshot"; onClicked: Quickshell.execDetached(["sh", "-c", "grim \"$HOME/Pictures/Screenshot-$(date +%F-%H%M%S).png\""]) }
                        Pill { text: "Capture Area"; onClicked: { win.collapse(); Quickshell.execDetached(["sh", "-c", "sleep 0.3; grim -g \"$(slurp)\" - | wl-copy --type image/png && notify-send -a Solyra 'Area copied' 'The capture is on the clipboard.'"]) } }
                        Pill { text: "Text from Screen"; onClicked: { win.collapse(); Quickshell.execDetached(["sh", "-c", "sleep 0.3; t=$(grim -g \"$(slurp)\" - | tesseract stdin stdout 2>/dev/null); [ -n \"$t\" ] && printf '%s' \"$t\" | wl-copy && notify-send -a Solyra 'Text copied' \"$t\""]) } }
                    }
                    // Termi-Notch: run a command without leaving your flow.
                    Rectangle {
                        width: parent.width; height: 34; radius: 10; color: Qt.rgba(1, 1, 1, 0.08)
                        Row {
                            anchors.fill: parent; anchors.leftMargin: 12; spacing: 8
                            Text { anchors.verticalCenter: parent.verticalCenter; text: "›"; color: "#7ee787"; font.family: Theme.mono; font.pixelSize: 15 }
                            TextField {
                                id: termField
                                anchors.verticalCenter: parent.verticalCenter
                                width: parent.width - 40
                                placeholderText: "Run a command"
                                color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.4)
                                font.family: Theme.mono; font.pixelSize: 13
                                background: null
                                onAccepted: { termOut.text = ""; termProc.command = ["sh", "-c", text]; termProc.running = true; text = "" }
                            }
                        }
                    }
                    Text {
                        id: termOut
                        width: parent.width; height: 70
                        color: Qt.rgba(1, 1, 1, 0.75); font.family: Theme.mono; font.pixelSize: 11
                        elide: Text.ElideRight; wrapMode: Text.WrapAnywhere; maximumLineCount: 5
                    }
                    Process {
                        id: termProc
                        stdout: StdioCollector { onStreamFinished: termOut.text = text.trim() }
                        stderr: StdioCollector { onStreamFinished: if (text.trim()) termOut.text += (termOut.text ? "\n" : "") + text.trim() }
                    }
                }
            }
        }
    }

    // Tabs pill floating under the open island.
    Rectangle {
        id: tabsPill
        anchors.horizontalCenter: shape.horizontalCenter
        y: shape.y + shape.height + 8
        width: tabsRow.width + 8; height: 34; radius: 17
        color: "black"
        opacity: win.mode === "expanded" ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }
        HoverHandler { onHoveredChanged: if (hovered) closeTimer.stop(); else closeTimer.restart() }
        Row {
            id: tabsRow
            anchors.centerIn: parent
            spacing: 2
            Repeater {
                model: [["media", "music"], ["shelf", "tray"], ["clip", "clipboard"], ["cal", "clock"], ["face", "eye"], ["tools", "terminal"]]
                Rectangle {
                    required property var modelData
                    width: 38; height: 26; radius: 13
                    color: win.tab === modelData[0] ? Qt.rgba(1, 1, 1, 0.18) : "transparent"
                    Icon { anchors.centerIn: parent; size: 15; name: modelData[1]; color: win.tab === modelData[0] ? "white" : Qt.rgba(1, 1, 1, 0.6) }
                    TapHandler { onTapped: win.tab = modelData[0] }
                }
            }
        }
    }

    property bool dropping: false

    // ------------------------------------------------------------ models
    ShelfModel { id: shelf }
    ClipHistory { id: clip }
    IdleInhibitor {
        id: caffeine
        window: win
        enabled: false
        function toggle() { enabled = !enabled; if (enabled) { caffeineTimer.left = 3600; caffeineTimer.start() } else caffeineTimer.stop() }
    }
    Timer {
        id: caffeineTimer
        property int left: 0
        interval: 1000; repeat: true
        onTriggered: { left -= 1; if (left <= 0) { caffeine.enabled = false; stop() } }
    }

    function expand(which) { tab = which; hovering = true; mode = "expanded"; closeTimer.stop() }
    function collapse() { hovering = false; settle() }
}
