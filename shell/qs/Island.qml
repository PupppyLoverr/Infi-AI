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
// - idle: a small black pill hanging from the top edge, centred;
// - compact: live activities. The first rides in the island's wings (album
//   art + equaliser, a timer counting down, High Alert, charging); a second
//   splits off into its own bubble beside it, as iOS does;
// - HUD: volume / brightness / battery / caps lock / Bluetooth, in the island;
// - Spotlight springs out of the island (Spotlight.qml);
// - expanded: pulls down from the top with a spring into tabs
//   (Media, Shelf, Clipboard, Calendar, Notchface, Tools).
// Idle, compact and HUD are pure black (the camera housing); expanded opens
// into dark Liquid Glass over Hyprland's refracting blur.
PanelWindow {
    id: win
    property var screenModel
    property var shell
    screen: screenModel
    anchors.top: true
    implicitWidth: 780
    implicitHeight: 480
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-island"
    WlrLayershell.keyboardFocus: termField.activeFocus ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None
    mask: Region {
        Region { item: shape }
        Region { item: bubble }
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

    // ------------------------------------------------------------ live activities
    // Everything running at once, most important first. The island shows the
    // first; the second gets its own bubble.
    readonly property real timerLeft: shell && shell.timerEnd > 0 ? Math.max(0, Math.ceil((shell.timerEnd - clock.now) / 1000)) : 0
    readonly property bool charging: !!(battery && battery.isLaptopBattery && battery.state === UPowerDeviceState.Charging)
    function mmss(s) { const m = Math.floor(s / 60); return m + ":" + String(s % 60).padStart(2, "0") }
    readonly property var activities: {
        const a = []
        if (shell && shell.recording) a.push({ kind: "rec", icon: "stop", tint: "#ff453a", label: mmss(Math.max(0, Math.floor((clock.now - shell.recordStart) / 1000))), tab: "tools" })
        if (timerLeft > 0) a.push({ kind: "timer", icon: "timer", tint: "#ff9f0a", label: mmss(timerLeft), tab: "tools" })
        if (playing) a.push({ kind: "media", tint: "#ff9f0a", tab: "media" })
        if (caffeine.enabled) a.push({ kind: "alert", icon: "eye", tint: "#ffd60a", label: Math.ceil(caffeineTimer.left / 60) + "m", tab: "tools" })
        if (charging) a.push({ kind: "charging", icon: "charging", tint: "#30d158", label: Math.round(battery.percentage * (battery.percentage <= 1 ? 100 : 1)) + "%", tab: "tools" })
        return a
    }
    readonly property var primary: activities.length > 0 ? activities[0] : null
    readonly property var secondary: activities.length > 1 ? activities[1] : null
    QtObject { id: clock; property real now: Date.now() }
    Timer { interval: 1000; repeat: true; running: win.shell && (win.shell.timerEnd > 0 || win.shell.recording); onTriggered: { clock.now = Date.now(); if (win.shell.timerEnd > 0 && clock.now >= win.shell.timerEnd) win.timerDone() } }
    function timerDone() {
        shell.timerEnd = 0
        Quickshell.execDetached(["notify-send", "-a", "Clock", "Timer", "Your timer is done."])
        hud("timer", 1, "Timer done")
    }

    function settle() {
        if (hovering || tab === "shelf" && dropping) { mode = "expanded"; return }
        if (hudTimer.running) { mode = "hud"; return }
        mode = activities.length > 0 ? "compact" : "idle"
    }
    function hud(kind, value, label) {
        hudKind = kind; hudValue = value; hudText = label || ""
        if (mode !== "expanded") { mode = "hud" }
        hudTimer.restart()
    }
    onActivitiesChanged: {
        console.info("island: live activities", JSON.stringify(activities.map(x => x.kind)))
        if (mode === "idle" || mode === "compact") settle()
    }
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
    // A MacBook-sized island: small at rest. Idle, live activities and HUDs
    // stay inside the 32 px menu bar; only the expanded tabs pull down past it.
    readonly property int barH: 32
    readonly property var dims: ({
        idle:     { w: 58, h: 22, r: 11 },
        compact:  { w: 210, h: 22, r: 11 },
        hud:      { w: 260, h: 22, r: 11 },
        expanded: { w: 500, h: 200, r: 34 }
    })
    // Media and the calendar glance are shorter than the card tabs: fit them.
    readonly property var d: mode === "expanded" && (tab === "media" || tab === "cal") ? { w: 500, h: 164, r: 32 }
        : mode === "expanded" && tab === "tools" ? { w: 540, h: 236, r: 34 }
        : dims[mode]

    Rectangle {
        id: shape
        // Idle, compact and HUD: solid black, the camera housing. Expanded:
        // dark Liquid Glass over Hyprland's refracting blur, with the
        // lensing rim; clear enough that the wallpaper shows through.
        readonly property bool glassy: win.mode === "expanded" && !Theme.reduceTransparency
        color: glassy ? Qt.rgba(0.07, 0.07, 0.09, 0.30 + Theme.glass / 100 * 0.45) : "black"
        Behavior on color { ColorAnimation { duration: Theme.ms(220) } }
        // Spotlight opens over the notch: the island steps aside meanwhile.
        opacity: (win.shell && win.shell.spotlightOpen) || (Theme.hideNotch && win.mode === "idle" && !hoverIdle.hovered) ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
        HoverHandler { id: hoverIdle }
        anchors.horizontalCenter: parent.horizontalCenter
        y: 4
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
        TapHandler {
            onTapped: {
                if (win.mode === "expanded") { win.hovering = false; win.settle() }
                else { if (win.primary) win.tab = win.primary.tab; win.hovering = true; win.mode = "expanded" }
            }
        }
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

        // Compact live activity: the primary's leading and trailing wings.
        Item {
            anchors.fill: parent
            opacity: win.mode === "compact" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }
            readonly property var act: win.primary
            ClippingRectangle {
                visible: parent.act && parent.act.kind === "media"
                x: 5; anchors.verticalCenter: parent.verticalCenter
                width: 16; height: 16; radius: 4; color: "#2c2c2e"
                Image { id: compactArt; anchors.fill: parent; source: win.player ? win.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
                // No artwork from the player: a music note, never an empty square.
                Icon { anchors.centerIn: parent; visible: compactArt.status !== Image.Ready; name: "music"; size: 10; color: "#ff9f0a" }
            }
            Icon {
                visible: parent.act && parent.act.kind !== "media"
                x: 8; anchors.verticalCenter: parent.verticalCenter
                size: 13; name: parent.act ? (parent.act.icon || "info") : "info"
                color: parent.act ? parent.act.tint : "white"
            }
            Equaliser { visible: parent.act && parent.act.kind === "media"; anchors.right: parent.right; anchors.rightMargin: 9; anchors.verticalCenter: parent.verticalCenter; active: win.playing; scale: 0.8 }
            Text {
                visible: parent.act && parent.act.kind !== "media"
                anchors.right: parent.right; anchors.rightMargin: 10; anchors.verticalCenter: parent.verticalCenter
                text: parent.act ? (parent.act.label || "") : ""
                color: parent.act ? parent.act.tint : "white"
                font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold; font.features: { "tnum": 1 }
            }
        }

        // HUD row.
        Row {
            anchors.centerIn: parent
            spacing: 10
            opacity: win.mode === "hud" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                size: 14
                name: ({ volume: "volume", mute: "volumeMute", brightness: "sun", battery: "battery", charging: "charging", capsOn: "up", capsOff: "up", bluetooth: "bluetooth", focus: "moon", timer: "timer" })[win.hudKind] || "info"
                color: win.hudKind === "charging" ? "#30d158" : win.hudKind === "timer" ? "#ff9f0a" : "white"
            }
            Text {
                visible: win.hudText !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: win.hudText + (win.hudKind === "bluetooth" ? "  · Connected" : "")
                color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
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
                width: 180; height: 5; radius: 2.5; color: Qt.rgba(1, 1, 1, 0.22)
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
                    spacing: 10
                    Flow {
                        width: parent.width
                        spacing: 8
                        Pill {
                            text: caffeine.enabled ? "High Alert · " + Math.ceil(caffeineTimer.left / 60) + " min" : "High Alert"
                            on: caffeine.enabled
                            onClicked: caffeine.toggle()
                        }
                        Pill {
                            text: win.timerLeft > 0 ? "Timer · " + win.mmss(win.timerLeft) + "  ✕" : "Timer 5 min"
                            on: win.timerLeft > 0
                            onClicked: if (win.shell) win.shell.timer(win.timerLeft > 0 ? 0 : 300)
                        }
                        Pill {
                            text: win.shell && win.shell.recording ? "Stop Recording" : "Record"
                            on: win.shell && win.shell.recording
                            onClicked: { win.collapse(); if (win.shell) win.shell.toggleRecording() }
                        }
                        Pill { text: "Lock"; onClicked: Quickshell.execDetached(["qs", "ipc", "call", "shell", "lock"]) }
                        Pill { text: "Screenshot"; onClicked: Quickshell.execDetached(["sh", "-c", "grim \"$HOME/Pictures/Screenshot-$(date +%F-%H%M%S).png\""]) }
                        Pill { text: "Markup"; onClicked: { win.collapse(); Quickshell.execDetached(["sh", "-c", "sleep 0.3; mkdir -p \"$HOME/Pictures\"; grim -g \"$(slurp)\" - | satty --filename - --output-filename \"$HOME/Pictures/Markup-$(date +%F-%H%M%S).png\" --copy-command wl-copy"]) } }
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
                        width: parent.width; height: 44
                        color: Qt.rgba(1, 1, 1, 0.75); font.family: Theme.mono; font.pixelSize: 11
                        elide: Text.ElideRight; wrapMode: Text.WrapAnywhere; maximumLineCount: 3
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

    // The second live activity splits off into its own bubble, springing out
    // of the island's right edge (and back in when it ends).
    Rectangle {
        id: bubble
        readonly property bool shown: win.mode === "compact" && win.secondary !== null
        width: 24; height: 24; radius: 12
        color: "black"
        y: shape.y
        x: shown ? shape.x + shape.width + 6 : shape.x + shape.width - width
        scale: shown ? 1 : 0.5
        opacity: shown ? 1 : 0
        visible: opacity > 0.01
        Behavior on x { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on scale { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.01 } }
        Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }
        readonly property var act: win.secondary
        ClippingRectangle {
            visible: bubble.act && bubble.act.kind === "media"
            anchors.centerIn: parent; width: 16; height: 16; radius: 8; color: "#2c2c2e"
            Image { id: bubbleArt; anchors.fill: parent; source: win.player ? win.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
            Icon { anchors.centerIn: parent; visible: bubbleArt.status !== Image.Ready; name: "music"; size: 9; color: "#ff9f0a" }
        }
        Icon {
            visible: bubble.act && bubble.act.kind !== "media"
            anchors.centerIn: parent; size: 12
            name: bubble.act ? (bubble.act.icon || "info") : "info"
            color: bubble.act ? bubble.act.tint : "white"
        }
        TapHandler { onTapped: if (bubble.act) win.expand(bubble.act.tab) }
    }


    // Tabs pill floating under the open island.
    Rectangle {
        id: tabsPill
        anchors.horizontalCenter: shape.horizontalCenter
        y: shape.y + shape.height + 8
        width: tabsRow.width + 8; height: 34; radius: 17
        // Dark Liquid Glass like the open island it belongs to.
        color: Theme.reduceTransparency ? "black" : Qt.rgba(0.07, 0.07, 0.09, 0.30 + Theme.glass / 100 * 0.45)
        border.width: Theme.reduceTransparency ? 0 : 1
        border.color: Qt.rgba(1, 1, 1, 0.16)
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
    // The clipboard history is the shell's (shared with Spotlight).
    readonly property var clip: shell ? shell.clipboard : null
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
