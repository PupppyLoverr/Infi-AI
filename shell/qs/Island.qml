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

// The complete Droppy + macOS 27 Dynamic Island:
// - Idle: crisp hardware notch with smooth concave outer ears flush to top bezel;
// - Compact / Tri-Island ("Three things at once"): Left circular Calendar ring
//   bubble + Center pill (Coding Agent spinner OR Media art + EQ) + Right circular
//   Focus Pomodoro ring bubble, plus 2-row Hover Peek expansion;
// - HUD: Volume / Brightness / Battery / Caps Lock + Concave-Ear AirPods Pro
//   Connected HUD with circular green `#30D158` battery ring (`80`);
// - Reply from the Island: inline quick-reply directly inside the notch;
// - Spotlight Search: smoked macOS 27 Liquid Glass search field + results;
// - Expanded Droppy Tabs & Widgets:
//     Media (+ [L] Synced Lyrics + Playing Next Queue), File Tray (+ Compression
//     Toast Pill & Floating Basket), Clipboard Manager, 2-Col Calendar Glance,
//     3-Col Multi-Widget Dashboard & Customizer, Pomodoro Tick Dial, Storage
//     Cleaner (Dot-Matrix LED), Per-App Audio Mixer, AI Usage Rings, Notchface,
//     and Tools + Quick Note.
PanelWindow {
    id: win
    property var screenModel
    property var shell
    property var shelf
    screen: screenModel
    anchors.top: true
    implicitWidth: 860
    implicitHeight: 540
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-island"
    WlrLayershell.keyboardFocus: mode === "reply" ? WlrKeyboardFocus.Exclusive
        : ((termField.activeFocus || noteField.activeFocus) ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None)
    mask: Region {
        Region { item: shape }
        Region { item: leftBubble }
        Region { item: rightBubble }
        Region { item: tabsRowHolder }
        Region { item: toastPill }
    }

    // ------------------------------------------------------------ state
    property string mode: "idle"         // idle | compact | hud | reply | expanded
    onModeChanged: if (shell) shell.islandExpanded = (mode === "expanded" || mode === "reply")
    property string tab: "media"         // media | shelf | widgets | cal | clip | pomodoro | cleaner | mixer | usage | face | tools
    property string hudKind: ""
    property string hudText: ""
    property real hudValue: 0
    property bool hovering: false
    property bool peekOpen: false
    property bool dropping: false
    property string toastTitle: ""
    property string toastDetail: ""

    readonly property MprisPlayer player: {
        const ps = Mpris.players.values
        let best = null
        for (const p of ps) { if (p.isPlaying) return p; if (!best) best = p }
        return best
    }
    readonly property bool playing: player !== null && player.isPlaying

    // ------------------------------------------------------------ live activities ("Three things at once")
    QtObject { id: clock; property real now: Date.now() }
    readonly property real timerLeft: shell && shell.timerEnd > 0 ? Math.max(0, Math.ceil((shell.timerEnd - clock.now) / 1000)) : 0
    readonly property real timerFrac: shell && shell.timerDuration > 0 && timerLeft > 0 ? Math.min(1, timerLeft / shell.timerDuration) : 0
    readonly property real eventLeft: shell && shell.nextEventEnd > 0 ? Math.max(0, Math.ceil((shell.nextEventEnd - clock.now) / 1000)) : 0
    readonly property bool agentRunning: shell !== null && shell.agentActive
    readonly property bool charging: battery && battery.isLaptopBattery && battery.state === UPowerDeviceState.Charging
    readonly property bool triMode: mode === "compact" && (eventLeft > 0 || agentRunning) && (timerLeft > 0 || playing || eventLeft > 0)

    function mmss(s) { const m = Math.floor(s / 60); return m + ":" + String(s % 60).padStart(2, "0") }

    readonly property var activities: {
        const a = []
        if (shell && shell.recording) a.push({ kind: "rec", icon: "stop", tint: "#ff453a", label: mmss(Math.max(0, Math.floor((clock.now - shell.recordStart) / 1000))), tab: "tools" })
        if (agentRunning) a.push({ kind: "agent", icon: "bot", tint: "#e07a5f", label: shell.agentLabel, tab: "usage" })
        if (timerLeft > 0) a.push({ kind: "timer", icon: "timer", tint: "#ff9f0a", label: mmss(timerLeft), tab: "pomodoro" })
        if (playing) a.push({ kind: "media", tint: "#ff7a45", tab: "media" })
        if (eventLeft > 0) a.push({ kind: "event", icon: "calendar", tint: "#ff5e57", label: mmss(eventLeft), tab: "cal" })
        if (caffeine.enabled) a.push({ kind: "alert", icon: "eye", tint: "#ffd60a", label: Math.ceil(caffeineTimer.left / 60) + "m", tab: "tools" })
        if (charging) a.push({ kind: "charging", icon: "charging", tint: "#30d158", label: Math.round(battery.percentage * (battery.percentage <= 1 ? 100 : 1)) + "%", tab: "tools" })
        return a
    }
    readonly property var primary: activities.length > 0 ? activities[0] : null
    readonly property var secondary: activities.length > 1 ? activities[1] : null

    Timer {
        interval: 1000; repeat: true
        running: win.shell && (win.shell.timerEnd > 0 || win.shell.recording || win.shell.nextEventEnd > 0 || win.agentRunning)
        onTriggered: {
            clock.now = Date.now()
            if (win.shell.timerEnd > 0 && clock.now >= win.shell.timerEnd) win.timerDone()
            if (win.shell.nextEventEnd > 0 && clock.now >= win.shell.nextEventEnd) win.shell.nextEventEnd = 0
            leftRingCanvas.requestPaint()
            rightRingCanvas.requestPaint()
        }
    }
    function timerDone() {
        shell.timerEnd = 0
        Quickshell.execDetached(["notify-send", "-a", "Clock", "Pomodoro", shell.pomodoroPhase === "focus" ? "Focus session complete — time for a break." : "Break is over."])
        hud("timer", 1, "Timer done")
        if (shell.pomodoroAutoBreak && shell.pomodoroPhase === "focus") {
            shell.pomodoroPhase = "break"
            shell.timer(5 * 60)
        }
    }

    function showToast(title, detail) {
        toastTitle = title || ""
        toastDetail = detail || ""
        toastTimer.restart()
    }
    Timer { id: toastTimer; interval: 4200; onTriggered: { win.toastTitle = ""; win.toastDetail = "" } }

    function settle() {
        if (mode === "reply") return
        if (hovering || (tab === "shelf" && dropping)) { mode = "expanded"; return }
        if (hudTimer.running) { mode = "hud"; return }
        mode = activities.length > 0 ? "compact" : "idle"
    }
    function hud(kind, value, label) {
        hudKind = kind; hudValue = value; hudText = label || ""
        if (mode !== "expanded" && mode !== "search") mode = "hud"
        btRingCanvas.requestPaint()
        hudTimer.restart()
    }
    function openReply() {
        if (!shell || !shell.replyBanner) return
        hovering = false; openTimer.stop(); closeTimer.stop()
        mode = "reply"
        replyInput.text = ""
        replyInput.forceActiveFocus()
    }
    function triggerShelfCompress() {
        if (shelf) shelf.compressSelected()
    }
    function triggerShelfSelectFirst() {
        if (!shelf || shelf.count === 0) return
        for (let i = 0; i < shelf.count; i++) {
            if (shelf.get(i).ext === "pdf") { shelf.selectOnly(i); return }
        }
        shelf.selectOnly(Math.min(1, shelf.count - 1))
    }

    onActivitiesChanged: if (mode === "idle" || mode === "compact") settle()
    onTriModeChanged: { leftRingCanvas.requestPaint(); rightRingCanvas.requestPaint() }
    onPeekOpenChanged: if (peekOpen && mode === "idle") mode = "compact"
    Timer { id: hudTimer; interval: 2200; onTriggered: win.settle() }
    Timer { id: openTimer; interval: 180; onTriggered: { win.hovering = true; win.mode = "expanded" } }
    Timer { id: closeTimer; interval: 400; onTriggered: { win.hovering = false; win.settle() } }
    Timer { interval: 1000; running: win.player !== null && win.playing; repeat: true; onTriggered: win.player.positionChanged() }

    // Connect shelf action toasts straight to the floating green-check toast pill!
    Connections {
        target: win.shelf
        function onActionToast(title, detail) { win.showToast(title, detail) }
    }

    // HUD sources: PipeWire volume, battery/charging, caps lock, BlueZ headphones + battery %.
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
            const chg = win.battery.state === UPowerDeviceState.Charging
            if (win.lastBatState !== -1) win.hud(chg ? "charging" : "battery", win.battery.percentage)
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

    // Bluetooth headphones: BlueZ "Connected" + Battery percentage
    Process {
        running: true
        command: ["sh", "-c", "dbus-monitor --system \"type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.bluez.Device1'\" 2>/dev/null | awk '/path=/{p=$0} /Connected/{getline; if ($0 ~ /true/) {match(p, /path=[^;]*/); print substr(p, RSTART+5, RLENGTH-5); fflush()}}'"]
        stdout: SplitParser {
            onRead: path => {
                btInfo.command = [
                    "sh", "-c",
                    "mac=$(echo \"$1\" | sed 's|.*dev_||; s|_|:|g'); info=$(bluetoothctl info \"$mac\" 2>/dev/null); " +
                    "name=$(printf '%s\\n' \"$info\" | sed -n 's/^\\s*Alias: //p' | head -1); " +
                    "bat=$(printf '%s\\n' \"$info\" | sed -n 's/.*Battery Percentage:.*(\\([0-9]*\\)).*/\\1/p' | head -1); " +
                    "printf '%s|%s\\n' \"${name:-AirPods Pro}\" \"${bat:-80}\"",
                    "sh", path.trim()
                ]
                btInfo.running = true
            }
        }
    }
    Process {
        id: btInfo
        stdout: StdioCollector {
            onStreamFinished: {
                const p = text.trim().split("|")
                win.hud("bluetooth", parseFloat(p[1]) || 80, p[0] || "AirPods Pro")
            }
        }
    }

    // Tracked Folders: new files in Downloads land on the File Tray automatically.
    Process {
        running: true
        command: ["sh", "-c", "command -v inotifywait >/dev/null || exit 0; inotifywait -m -q -e close_write -e moved_to --format '%w%f' \"$HOME/Downloads\""]
        stdout: SplitParser { onRead: f => { if (f && !/\.(part|crdownload|tmp)$/.test(f) && win.shelf) win.shelf.add("file://" + f, false) } }
    }

    // ------------------------------------------------------------ geometry
    readonly property bool isAirpodsHud: mode === "hud" && (hudKind === "bluetooth" || hudKind === "airpods")
    readonly property bool flushNotch: isAirpodsHud || (mode === "expanded" && (tab === "usage" || tab === "pomodoro"))
    readonly property bool frostedBottom: mode === "expanded" && (tab === "cal" || tab === "shelf") && !Theme.reduceTransparency

    readonly property var d: mode === "reply" ? { w: 432, h: 98, r: 24 }
        : isAirpodsHud ? { w: 308, h: 64, r: 22 }
        : mode === "compact" && peekOpen ? { w: 236, h: 58, r: 18 }
        : mode === "compact" && triMode ? { w: 122, h: 32, r: 16 }
        : mode === "compact" ? { w: 242, h: 25, r: 12 }
        : mode === "hud" ? { w: 284, h: 25, r: 12 }
        : mode === "expanded" && tab === "media" ? {
            w: (mediaView.lyricsOpen || mediaView.queueOpen) ? 356 : 384,
            h: mediaView.queueOpen ? 364 : (mediaView.lyricsOpen ? 324 : 164),
            r: 28
        }
        : mode === "expanded" && tab === "clip" ? { w: 664, h: 228, r: 26 }
        : mode === "expanded" && tab === "shelf" ? { w: 580, h: 204, r: 26 }
        : mode === "expanded" && tab === "cal" ? { w: 510, h: 196, r: 26 }
        : mode === "expanded" && tab === "widgets" ? { w: 720, h: widgetsDash.customizing ? 286 : 216, r: 28 }
        : mode === "expanded" && tab === "pomodoro" ? { w: 410, h: 176, r: 26 }
        : mode === "expanded" && tab === "cleaner" ? { w: 446, h: 226, r: 26 }
        : mode === "expanded" && tab === "mixer" ? { w: 420, h: Math.min(312, Math.max(176, 86 + audioMixer.streams.length * 45)), r: 26 }
        : mode === "expanded" && tab === "usage" ? { w: 396, h: 184, r: 26 }
        : mode === "expanded" && tab === "tools" ? { w: 540, h: 256, r: 28 }
        : mode === "expanded" ? { w: 480, h: 196, r: 28 }
        : { w: hoverIdle.hovered ? 134 : 126, h: 25, r: 12.5 }

    // Concave outer hardware ears when flush to the top bezel (exact match to 07, 16, 30, 31!)
    Shape {
        visible: win.flushNotch
        x: shape.x - 14
        y: 0
        width: shape.width + 28
        height: 20
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            fillColor: "#000000"
            strokeWidth: 0
            startX: 0; startY: 0
            PathLine { x: 16; y: 0 }
            PathLine { x: 16; y: 16 }
            PathQuad { x: 0; y: 0; controlX: 14; controlY: 0 }
        }
        ShapePath {
            fillColor: "#000000"
            strokeWidth: 0
            startX: shape.width + 28; startY: 0
            PathLine { x: shape.width + 12; y: 0 }
            PathLine { x: shape.width + 12; y: 16 }
            PathQuad { x: shape.width + 28; y: 0; controlX: shape.width + 14; controlY: 0 }
        }
    }

    Rectangle {
        id: shape
        color: win.frostedBottom ? "transparent" : "#000000"
        Behavior on color { ColorAnimation { duration: Theme.ms(220) } }
        opacity: Theme.hideNotch && win.mode === "idle" && !hoverIdle.hovered ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
        HoverHandler { id: hoverIdle }
        anchors.horizontalCenter: parent.horizontalCenter
        y: win.flushNotch ? 0 : 4
        width: win.d.w
        height: win.d.h
        radius: win.d.r
        clip: true
        border.width: win.mode === "expanded" && !win.flushNotch ? 1 : 0
        border.color: Qt.rgba(1, 1, 1, win.frostedBottom ? 0.16 : 0.11)
        Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: win.mode === "expanded" ? Theme.springOpen : Theme.springClose; damping: win.mode === "expanded" ? Theme.dampOpen : Theme.dampClose; epsilon: 0.3 } }
        Behavior on height { enabled: !Theme.reduceMotion; SpringAnimation { spring: win.mode === "expanded" ? Theme.springOpen : Theme.springClose; damping: win.mode === "expanded" ? Theme.dampOpen : Theme.dampClose; epsilon: 0.3 } }
        Behavior on radius { NumberAnimation { duration: Theme.ms(240); easing.type: Easing.OutCubic } }

        // Droppy signature dark-to-frosted-glass bottom gradient fade (Calendar & File Tray)
        Rectangle {
            anchors.fill: parent
            radius: shape.radius
            visible: win.frostedBottom
            gradient: Gradient {
                GradientStop { position: 0.0;  color: Qt.rgba(0.0, 0.0, 0.0, 0.98) }
                GradientStop { position: 0.56; color: Qt.rgba(0.02, 0.02, 0.04, 0.92) }
                GradientStop { position: 1.0;  color: Qt.rgba(0.14, 0.16, 0.22, 0.58) }
            }
        }

        // Flush top corners square-off when attached to top bezel
        Rectangle {
            visible: win.flushNotch
            anchors { top: parent.top; left: parent.left; right: parent.right }
            height: 18
            color: "#000000"
        }

        HoverHandler {
            enabled: win.mode !== "reply"
            onHoveredChanged: {
                if (hovered) { closeTimer.stop(); openTimer.restart() }
                else { openTimer.stop(); closeTimer.restart() }
            }
        }
        TapHandler {
            enabled: win.mode !== "reply"
            onTapped: {
                if (win.mode === "expanded") { win.hovering = false; win.settle() }
                else { if (win.primary) win.tab = win.primary.tab; win.hovering = true; win.mode = "expanded" }
            }
        }
        DropArea {
            anchors.fill: parent
            onEntered: { win.tab = "shelf"; win.dropping = true; win.hovering = true; win.mode = "expanded" }
            onExited: win.dropping = false
            onDropped: drop => { win.dropping = false; for (const u of drop.urls) if (win.shelf) win.shelf.add(u.toString(), false) }
        }

        // ------------------------------------------------ COMPACT LIVE ACTIVITY (Normal, Tri-Island Center, or 2-Row Hover Peek)
        Item {
            anchors.fill: parent
            visible: win.mode === "compact"
            opacity: win.mode === "compact" ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }

            // Mode A: 2-Row Hover Peek (exact match to 20_droppy_three_things_hover_peek.png)
            Column {
                visible: win.peekOpen
                anchors.fill: parent
                anchors.leftMargin: 12; anchors.rightMargin: 12; anchors.topMargin: 7; anchors.bottomMargin: 7
                spacing: 6
                Item {
                    width: parent.width; height: 18
                    Icon { anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; name: "calendar"; size: 15; color: "#ff5e57" }
                    Text {
                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                        text: win.eventLeft > 0 ? win.mmss(win.eventLeft) : (win.timerLeft > 0 ? win.mmss(win.timerLeft) : "4:59")
                        color: "#ff5e57"
                        font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold; font.features: { "tnum": 1 }
                    }
                }
                Row {
                    width: parent.width; spacing: 8
                    Text {
                        width: parent.width - 84
                        elide: Text.ElideRight
                        text: win.shell ? win.shell.nextEventTitle : "Design review"
                        color: Qt.rgba(1, 1, 1, 0.62)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                    }
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "clock"; size: 13; color: Qt.rgba(1, 1, 1, 0.45) }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: win.eventLeft > 0 ? win.mmss(win.eventLeft) : "4:59"
                        color: Qt.rgba(1, 1, 1, 0.56)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold; font.features: { "tnum": 1 }
                    }
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "calendar"; size: 13; color: Qt.rgba(1, 1, 1, 0.34) }
                }
            }

            // Mode B: Tri-Island Center Pill (coral coding-agent glyph left + circular spinner right — 21_droppy_three_things_tri_island.png)
            Item {
                visible: !win.peekOpen && win.triMode
                anchors.fill: parent
                Icon {
                    x: 11; anchors.verticalCenter: parent.verticalCenter
                    name: win.agentRunning ? "bot" : "music"
                    size: 16; color: "#e07a5f"
                }
                Canvas {
                    id: spinCanvas
                    anchors.right: parent.right; anchors.rightMargin: 9
                    anchors.verticalCenter: parent.verticalCenter
                    width: 18; height: 18
                    property real angle: 0
                    Timer { interval: 55; running: win.visible && win.triMode && !win.peekOpen && !Theme.reduceMotion; repeat: true; onTriggered: { spinCanvas.angle = (spinCanvas.angle + 0.24) % (Math.PI * 2); spinCanvas.requestPaint() } }
                    onPaint: {
                        const ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        ctx.lineWidth = 2
                        ctx.strokeStyle = "rgba(224,122,95,0.24)"
                        ctx.beginPath(); ctx.arc(9, 9, 6.5, 0, Math.PI * 2); ctx.stroke()
                        ctx.strokeStyle = "#e07a5f"
                        ctx.lineCap = "round"
                        ctx.beginPath(); ctx.arc(9, 9, 6.5, angle, angle + 1.5); ctx.stroke()
                    }
                }
            }

            // Mode C: Standard Compact Activity (album art + EQ, timer, recording, charging)
            Item {
                visible: !win.peekOpen && !win.triMode
                anchors.fill: parent
                readonly property var act: win.primary
                ClippingRectangle {
                    visible: parent.act && parent.act.kind === "media"
                    x: 5; anchors.verticalCenter: parent.verticalCenter
                    width: 17; height: 17; radius: 4; color: "#25252b"
                    Icon { visible: !win.player || !win.player.trackArtUrl; anchors.centerIn: parent; name: "music"; size: 11; color: "#ff7a45" }
                    Image { anchors.fill: parent; source: win.player ? win.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
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
        }

        // ------------------------------------------------ AIRPODS / BLUETOOTH BATTERY RING HUD (07, 16)
        Item {
            anchors.fill: parent
            anchors.leftMargin: 18; anchors.rightMargin: 18
            visible: win.isAirpodsHud
            opacity: win.isAirpodsHud ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }

            Row {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                spacing: 14
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "airpods"; size: 30; color: "white"
                }
                Column {
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 1
                    Text {
                        text: "Connected"
                        color: Qt.rgba(1, 1, 1, 0.65)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                    }
                    Text {
                        text: win.hudText || "AirPods Pro"
                        color: "white"
                        font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Bold
                    }
                }
            }

            // Crisp green circular battery ring (#30D158) with bold numeric % centered inside
            Item {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                width: 38; height: 38
                readonly property int pct: Math.max(0, Math.min(100, Math.round(win.hudValue > 1.001 ? win.hudValue : win.hudValue * 100)))
                Canvas {
                    id: btRingCanvas
                    anchors.fill: parent
                    onVisibleChanged: if (visible) requestPaint()
                    onPaint: {
                        const ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        const cx = width / 2, cy = height / 2, rad = 16
                        const pct = Math.max(0, Math.min(100, Math.round(win.hudValue > 1.001 ? win.hudValue : win.hudValue * 100)))
                        ctx.lineWidth = 3.2
                        ctx.lineCap = "round"
                        ctx.strokeStyle = "rgba(255,255,255,0.16)"
                        ctx.beginPath(); ctx.arc(cx, cy, rad, 0, Math.PI * 2); ctx.stroke()
                        ctx.strokeStyle = pct <= 20 ? "#ff453a" : "#30d158"
                        ctx.beginPath(); ctx.arc(cx, cy, rad, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * (pct / 100)); ctx.stroke()
                    }
                }
                Text {
                    anchors.centerIn: parent
                    text: parent.pct
                    color: parent.pct <= 20 ? "#ff453a" : "#30d158"
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold; font.features: { "tnum": 1 }
                }
            }
        }

        // ------------------------------------------------ STANDARD SLIM HUD ROW (Volume / Brightness / Caps / Battery)
        Row {
            anchors.centerIn: parent
            spacing: 10
            visible: win.mode === "hud" && !win.isAirpodsHud
            opacity: win.mode === "hud" && !win.isAirpodsHud ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                size: 14
                name: ({ volume: "volume", mute: "volumeMute", brightness: "sun", battery: "battery", charging: "charging", capsOn: "up", capsOff: "up", focus: "moon", timer: "timer" })[win.hudKind] || "info"
                color: win.hudKind === "charging" ? "#30d158" : win.hudKind === "timer" ? "#ff9f0a" : "white"
            }
            Text {
                visible: win.hudText !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: win.hudText
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

        // ------------------------------------------------ REPLY FROM THE ISLAND (29)
        Column {
            visible: win.mode === "reply"
            anchors.fill: parent
            anchors.margins: 12
            spacing: 8

            Item {
                width: parent.width; height: 34
                Row {
                    anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; spacing: 10
                    Rectangle {
                        width: 30; height: 30; radius: 15
                        gradient: Gradient {
                            GradientStop { position: 0; color: "#34d399" }
                            GradientStop { position: 1; color: "#059669" }
                        }
                        Text {
                            anchors.centerIn: parent
                            text: win.shell && win.shell.replyBanner ? win.shell.replyBanner.sender.charAt(0).toUpperCase() : "M"
                            color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                        }
                    }
                    Column {
                        width: shape.width - 104
                        spacing: 1
                        Row {
                            spacing: 6
                            Text { text: win.shell && win.shell.replyBanner ? win.shell.replyBanner.sender : "Message"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold }
                            Text { text: win.shell && win.shell.replyBanner ? ("· " + win.shell.replyBanner.app) : ""; color: Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 11 }
                        }
                        Text {
                            width: parent.width; elide: Text.ElideRight
                            text: win.shell && win.shell.replyBanner ? win.shell.replyBanner.message : ""
                            color: Qt.rgba(1, 1, 1, 0.78); font.family: Theme.font; font.pixelSize: 12
                        }
                    }
                }
                Rectangle {
                    anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                    width: 22; height: 22; radius: 11; color: Qt.rgba(1, 1, 1, 0.14)
                    Icon { anchors.centerIn: parent; name: "x"; size: 11; color: "white" }
                    TapHandler { onTapped: { if (win.shell) win.shell.replyBanner = null; win.mode = "idle"; win.settle() } }
                }
            }

            Row {
                width: parent.width; spacing: 8
                Rectangle {
                    width: parent.width - 38; height: 32; radius: 16
                    color: Qt.rgba(1, 1, 1, 0.12)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
                    TextField {
                        id: replyInput
                        anchors.fill: parent; anchors.leftMargin: 12; anchors.rightMargin: 12
                        placeholderText: "Reply from the island…"
                        color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.45)
                        font.family: Theme.font; font.pixelSize: 12
                        background: null
                        Keys.onEscapePressed: { if (win.shell) win.shell.replyBanner = null; win.mode = "idle"; win.settle() }
                        onAccepted: {
                            if (win.shell) win.shell.sendIslandReply(text)
                            text = ""
                            win.mode = "idle"; win.settle()
                        }
                    }
                }
                Rectangle {
                    width: 32; height: 32; radius: 16
                    color: "#0a84ff"
                    Icon { anchors.centerIn: parent; name: "send"; size: 14; color: "white" }
                    TapHandler {
                        onTapped: {
                            if (win.shell) win.shell.sendIslandReply(replyInput.text || "On my way!")
                            replyInput.text = ""
                            win.mode = "idle"; win.settle()
                        }
                    }
                }
            }
        }

        // ------------------------------------------------ EXPANDED DROPPY CONTENT
        Item {
            id: content
            anchors.fill: parent
            anchors.margins: 18
            anchors.topMargin: win.flushNotch ? 14 : 16
            opacity: win.mode === "expanded" && shape.height > 120 ? 1 : 0
            scale: opacity > 0 ? 1 : 0.96
            Behavior on opacity { NumberAnimation { duration: Theme.ms(200); easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: Theme.ms(240); easing.type: Easing.OutCubic } }

            MediaView {
                id: mediaView
                anchors.fill: parent
                visible: win.tab === "media"
                player: win.player
                lyrics: win.shell ? win.shell.lyrics : null
                shell: win.shell
                onExpandLock: { win.collapse(); if (win.shell) win.shell.lock() }
                onToast: (t, d) => win.showToast(t, d)
            }
            ShelfView {
                id: shelfView
                anchors.fill: parent
                visible: win.tab === "shelf"
                model: win.shelf
                shell: win.shell
                dropping: win.dropping
            }
            ClipView { anchors.fill: parent; visible: win.tab === "clip"; history: win.shell ? win.shell.clipboard : clip }
            CalendarGlance { anchors.fill: parent; visible: win.tab === "cal"; shell: win.shell }
            WidgetsDashboard {
                id: widgetsDash
                anchors.fill: parent
                visible: win.tab === "widgets"
                shell: win.shell
                player: win.player
                onOpenWidget: wtab => { win.tab = wtab }
            }
            PomodoroWidget { anchors.fill: parent; visible: win.tab === "pomodoro"; shell: win.shell }
            StorageCleanerWidget {
                anchors.fill: parent
                visible: win.tab === "cleaner"
                onToast: (t, d) => win.showToast(t, d)
            }
            AudioMixerWidget {
                id: audioMixer
                anchors.fill: parent
                visible: win.tab === "mixer"
            }
            UsageWidget { anchors.fill: parent; visible: win.tab === "usage" }
            Loader {
                anchors.fill: parent
                active: win.tab === "face"
                source: "NotchFace.qml"
                onLoaded: item.active = Qt.binding(() => win.tab === "face" && win.mode === "expanded")
                onStatusChanged: if (status === Loader.Error) faceError.visible = true
            }
            Text { id: faceError; visible: false; anchors.centerIn: parent; text: "Notchface needs QtMultimedia"; color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 13 }

            // Tools + Termi-Notch + Droppy Quick Note
            Item {
                anchors.fill: parent
                visible: win.tab === "tools"
                Column {
                    anchors.fill: parent
                    spacing: 10
                    Flow {
                        width: parent.width
                        spacing: 8
                        Pill { text: caffeine.enabled ? "High Alert · " + Math.ceil(caffeineTimer.left / 60) + " min" : "High Alert"; on: caffeine.enabled; onClicked: caffeine.toggle() }
                        Pill { text: "Pomodoro"; on: win.timerLeft > 0; onClicked: win.tab = "pomodoro" }
                        Pill { text: "Cleaner"; onClicked: win.tab = "cleaner" }
                        Pill { text: "Audio Mixer"; onClicked: win.tab = "mixer" }
                        Pill { text: "AI Usage"; onClicked: win.tab = "usage" }
                        Pill { text: win.shell && win.shell.recording ? "Stop Recording" : "Record"; on: win.shell && win.shell.recording; onClicked: { win.collapse(); if (win.shell) win.shell.toggleRecording() } }
                        Pill { text: "Notchface"; onClicked: win.tab = "face" }
                        Pill { text: "Lock"; onClicked: Quickshell.execDetached(["qs", "ipc", "call", "shell", "lock"]) }
                        Pill { text: "Screenshot"; onClicked: Quickshell.execDetached(["sh", "-c", "grim \"$HOME/Pictures/Screenshot-$(date +%F-%H%M%S).png\""]) }
                    }
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
                    Rectangle {
                        width: parent.width; height: 32; radius: 10; color: Qt.rgba(1, 0.84, 0.35, 0.10)
                        border.width: 1; border.color: Qt.rgba(1, 0.84, 0.35, 0.22)
                        Row {
                            anchors.fill: parent; anchors.leftMargin: 10; spacing: 8
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "document"; size: 13; color: "#ffd60a" }
                            TextField {
                                id: noteField
                                anchors.verticalCenter: parent.verticalCenter
                                width: parent.width - 34
                                placeholderText: "Quick note…"
                                color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.42)
                                font.family: Theme.font; font.pixelSize: 12
                                background: null
                                onEditingFinished: Quickshell.execDetached(["sh", "-c", "mkdir -p \"$HOME/.local/share/solyra\" && printf '%s' \"$1\" > \"$HOME/.local/share/solyra/quicknote.txt\"", "sh", text])
                            }
                        }
                        FileView {
                            path: Theme.home + "/.local/share/solyra/quicknote.txt"
                            onLoaded: if (noteField.text === "") noteField.text = text()
                        }
                    }
                    Text {
                        id: termOut
                        width: parent.width; height: 28
                        color: Qt.rgba(1, 1, 1, 0.75); font.family: Theme.mono; font.pixelSize: 11
                        elide: Text.ElideRight; wrapMode: Text.WrapAnywhere; maximumLineCount: 2
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

    // ------------------------------------------------ TRI-ISLAND LEFT CIRCULAR BUBBLE (Calendar Countdown Ring — 21)
    Rectangle {
        id: leftBubble
        readonly property bool shown: win.mode === "compact" && !win.peekOpen && win.triMode && win.eventLeft > 0
        width: 32; height: 32; radius: 16
        color: "#000000"
        y: shape.y
        x: shown ? shape.x - width - 7 : shape.x
        scale: shown ? 1 : 0.5
        opacity: shown ? 1 : 0
        visible: opacity > 0.01
        Behavior on x { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on scale { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.01 } }
        Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }

        Canvas {
            id: leftRingCanvas
            anchors.fill: parent
            onVisibleChanged: if (visible) requestPaint()
            onPaint: {
                const ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.lineWidth = 2.2
                ctx.strokeStyle = "rgba(255,94,87,0.28)"
                ctx.beginPath(); ctx.arc(16, 16, 12.5, 0, Math.PI * 2); ctx.stroke()
                ctx.strokeStyle = "#ff5e57"
                ctx.lineCap = "round"
                const f = Math.max(0.15, Math.min(1, win.eventLeft / 300))
                ctx.beginPath(); ctx.arc(16, 16, 12.5, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * f); ctx.stroke()
            }
        }
        Icon { anchors.centerIn: parent; name: "calendar"; size: 13; color: "#ff5e57" }
        TapHandler { onTapped: win.expand("cal") }
    }

    // ------------------------------------------------ RIGHT CIRCULAR BUBBLE (Focus Timer Ring OR Secondary Activity — 21)
    Rectangle {
        id: rightBubble
        readonly property bool shown: win.mode === "compact" && !win.peekOpen && ((win.triMode && win.timerLeft > 0) || (!win.triMode && win.secondary !== null))
        width: win.triMode ? 32 : 24
        height: win.triMode ? 32 : 24
        radius: width / 2
        color: "#000000"
        y: shape.y
        x: shown ? shape.x + shape.width + 7 : shape.x + shape.width - width
        scale: shown ? 1 : 0.5
        opacity: shown ? 1 : 0
        visible: opacity > 0.01
        Behavior on x { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.3 } }
        Behavior on scale { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen; epsilon: 0.01 } }
        Behavior on opacity { NumberAnimation { duration: Theme.ms(160) } }

        Canvas {
            id: rightRingCanvas
            visible: win.triMode
            anchors.fill: parent
            onVisibleChanged: if (visible) requestPaint()
            onPaint: {
                const ctx = getContext("2d")
                ctx.clearRect(0, 0, width, height)
                ctx.lineWidth = 2.2
                ctx.strokeStyle = "rgba(255,159,10,0.28)"
                ctx.beginPath(); ctx.arc(16, 16, 12.5, 0, Math.PI * 2); ctx.stroke()
                ctx.strokeStyle = "#ff9f0a"
                ctx.lineCap = "round"
                const f = Math.max(0.18, win.timerFrac || 0.78)
                ctx.beginPath(); ctx.arc(16, 16, 12.5, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * f); ctx.stroke()
            }
        }
        readonly property var act: win.triMode ? { kind: "timer", icon: "timer", tint: "#ff9f0a", tab: "pomodoro" } : win.secondary
        ClippingRectangle {
            visible: rightBubble.act && rightBubble.act.kind === "media"
            anchors.centerIn: parent; width: 17; height: 17; radius: 8; color: "#25252b"
            Icon { visible: !win.player || !win.player.trackArtUrl; anchors.centerIn: parent; name: "music"; size: 11; color: "#ff7a45" }
            Image { anchors.fill: parent; source: win.player ? win.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
        }
        Icon {
            visible: rightBubble.act && rightBubble.act.kind !== "media"
            anchors.centerIn: parent; size: win.triMode ? 13 : 12
            name: rightBubble.act ? (rightBubble.act.icon || "info") : "info"
            color: rightBubble.act ? rightBubble.act.tint : "white"
        }
        TapHandler { onTapped: if (rightBubble.act) win.expand(rightBubble.act.tab) }
    }

    // ------------------------------------------------ DETACHED MODE SWITCHER PILL + DETACHED × CLOSE BUTTON (17, 23, 24, 25, 27)
    Row {
        id: tabsRowHolder
        anchors.horizontalCenter: shape.horizontalCenter
        y: shape.y + shape.height + 9
        spacing: 8
        opacity: win.mode === "expanded" ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }
        HoverHandler { onHoveredChanged: if (hovered) closeTimer.stop(); else closeTimer.restart() }

        Rectangle {
            height: 32; width: tabsRow.width + 10; radius: 16
            color: win.frostedBottom ? Qt.rgba(0.10, 0.11, 0.14, 0.72) : "#000000"
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, win.frostedBottom ? 0.22 : 0.14)

            Row {
                id: tabsRow
                anchors.centerIn: parent
                spacing: 3

                // Home / Media
                Rectangle {
                    width: 34; height: 24; radius: 12
                    color: win.tab === "media" ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                    Icon { anchors.centerIn: parent; size: 14; name: "home"; color: win.tab === "media" ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                    TapHandler { onTapped: win.tab = "media" }
                }
                // File Tray (+ live count pill badge when non-empty!)
                Rectangle {
                    readonly property int cnt: win.shelf ? win.shelf.count : 0
                    width: cnt > 0 ? 50 : 34; height: 24; radius: 12
                    color: win.tab === "shelf" ? Qt.rgba(1, 1, 1, 0.24) : "transparent"
                    Row {
                        anchors.centerIn: parent; spacing: 5
                        Icon { anchors.verticalCenter: parent.verticalCenter; size: 14; name: "tray"; color: win.tab === "shelf" ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                        Rectangle {
                            visible: parent.parent.cnt > 0
                            anchors.verticalCenter: parent.verticalCenter
                            width: 16; height: 16; radius: 8
                            color: Qt.rgba(1, 1, 1, 0.24)
                            Text { anchors.centerIn: parent; text: parent.parent.parent.cnt; color: "white"; font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Bold }
                        }
                    }
                    TapHandler { onTapped: win.tab = "shelf" }
                }
                // Widgets Dashboard (also lights up for pomodoro/cleaner/mixer/usage)
                Rectangle {
                    id: widgetsTabBtn
                    readonly property bool act: win.tab === "widgets" || win.tab === "pomodoro" || win.tab === "cleaner" || win.tab === "mixer" || win.tab === "usage"
                    width: 34; height: 24; radius: 12
                    color: widgetsTabBtn.act ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                    Icon { anchors.centerIn: parent; size: 14; name: "grid"; color: widgetsTabBtn.act ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                    TapHandler { onTapped: win.tab = "widgets" }
                }
                // Subtle Droppy divider
                Rectangle { width: 1; height: 14; anchors.verticalCenter: parent.verticalCenter; color: Qt.rgba(1, 1, 1, 0.18) }
                // Calendar
                Rectangle {
                    width: 34; height: 24; radius: 12
                    color: win.tab === "cal" ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                    Icon { anchors.centerIn: parent; size: 14; name: "calendar"; color: win.tab === "cal" ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                    TapHandler { onTapped: win.tab = "cal" }
                }
                // Clipboard
                Rectangle {
                    width: 34; height: 24; radius: 12
                    color: win.tab === "clip" ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                    Icon { anchors.centerIn: parent; size: 14; name: "clipboard"; color: win.tab === "clip" ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                    TapHandler { onTapped: win.tab = "clip" }
                }
                // Tools / Termi-Notch
                Rectangle {
                    width: 34; height: 24; radius: 12
                    color: win.tab === "tools" || win.tab === "face" ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                    Icon { anchors.centerIn: parent; size: 14; name: "terminal"; color: win.tab === "tools" || win.tab === "face" ? "white" : Qt.rgba(1, 1, 1, 0.60) }
                    TapHandler { onTapped: win.tab = "tools" }
                }
            }
        }

        // Detached circular `×` close button (exact match to 23_droppy_storage_cleaner_widget.png!)
        Rectangle {
            width: 32; height: 32; radius: 16
            color: win.frostedBottom ? Qt.rgba(0.10, 0.11, 0.14, 0.72) : "#000000"
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, win.frostedBottom ? 0.22 : 0.14)
            Icon { anchors.centerIn: parent; name: "x"; size: 13; color: "white" }
            TapHandler { onTapped: win.collapse() }
        }
    }

    // ------------------------------------------------ FLOATING GREEN-CHECK TOAST PILL (25_droppy_file_tray_compressed_toast.png)
    Rectangle {
        id: toastPill
        anchors.horizontalCenter: shape.horizontalCenter
        y: (win.mode === "expanded" ? tabsRowHolder.y + tabsRowHolder.height + 10 : shape.y + shape.height + 12)
        height: 38
        width: toastRow.implicitWidth + 28
        radius: 19
        color: Qt.rgba(0.08, 0.10, 0.14, 0.88)
        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.18)
        opacity: win.toastTitle !== "" ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(180) } }

        Row {
            id: toastRow
            anchors.centerIn: parent
            spacing: 10
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: 22; height: 22; radius: 11
                color: "#154026"
                Rectangle {
                    anchors.centerIn: parent
                    width: 15; height: 15; radius: 8
                    color: "#30d158"
                    Icon { anchors.centerIn: parent; name: "check"; size: 10; color: "#081c10" }
                }
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: win.toastTitle
                color: "white"
                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
            }
            Text {
                visible: win.toastDetail !== ""
                anchors.verticalCenter: parent.verticalCenter
                text: win.toastDetail
                color: Qt.rgba(1, 1, 1, 0.72)
                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
                font.features: { "tnum": 1 }
            }
        }
    }

    // ------------------------------------------------------------ models
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

    function expand(which) {
        peekOpen = false
        if (which === "lyrics") {
            tab = "media"
            mediaView.lyricsOpen = true
            mediaView.queueOpen = false
        } else if (which === "queue") {
            tab = "media"
            mediaView.lyricsOpen = false
            mediaView.queueOpen = true
        } else if (which === "customize") {
            tab = "widgets"
            widgetsDash.draftWidgets = widgetsDash.activeWidgets.slice()
            widgetsDash.customizing = true
        } else {
            tab = which
            if (which === "media") { mediaView.lyricsOpen = false; mediaView.queueOpen = false }
            if (which === "widgets") widgetsDash.customizing = false
        }
        hovering = true; mode = "expanded"; closeTimer.stop()
    }
    function collapse() {
        if (mode === "reply") { if (shell) shell.replyBanner = null; mode = "idle" }
        peekOpen = false
        hovering = false
        settle()
    }
}
