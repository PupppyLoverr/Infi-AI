import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pam
import Quickshell.Services.Pipewire
import Quickshell.Services.UPower
import Quickshell.Widgets

// Exact getdroppy.app 3-Mode Fullscreen Ambient Lock Screen (ext-session-lock + PAM):
//  1) "depth"  — Full Art & Vision Clock Depth: edge-to-edge ambient artwork with
//     crisp center portrait art, single-line clock right below the hardware notch
//     (`14:30  Mon, 23 Nov`), and centered lower Liquid Glass media controls.
//  2) "lyrics" — Live Lyrics: split-screen large album art + controls on left,
//     6-line glowing synced lyrics on right.
//  3) "queue"  — Playing Next: split-screen large album art + controls on left,
//     "Playing Next / Continue Playing" queue + Shuffle/Repeat buttons on right.
// Bottom corners: circular frosted Output Device button left, 3-mode segmented
// glass pill (`Depth | Lyrics | Queue`) right, and real PAM unlock center.
Scope {
    id: root
    property var shell
    property var lyrics
    property bool locked: false
    property string wallpaper: Theme.home + "/.config/solyra/wallpaper.png"
    property string status: ""
    property bool unlocking: false
    property bool shuffleOn: false
    property string loopMode: "None"
    property bool favOn: false

    readonly property string lockMode: shell ? shell.lockMode : "depth"
    readonly property bool signInOpen: shell ? shell.lockSignInOpen : false
    readonly property var queueItems: shell ? shell.mediaQueue : []

    function openSignIn() { if (root.shell) root.shell.setLockSignIn(true) }
    function closeSignIn() { if (root.shell) root.shell.setLockSignIn(false) }

    readonly property MprisPlayer player: {
        for (const p of Mpris.players.values) if (p.isPlaying) return p
        return Mpris.players.values.length ? Mpris.players.values[0] : null
    }
    PwObjectTracker { objects: [Pipewire.defaultAudioSink] }
    readonly property var sink: Pipewire.defaultAudioSink

    function lock() {
        status = ""
        unlocking = false
        locked = true
        if (root.lyrics) root.lyrics.refresh()
    }

    function setMode(m) {
        if (root.shell) root.shell.setLockMode(m)
    }

    function fmt(s) {
        if (!(s >= 0)) return "--:--"
        s = Math.floor(s)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    function toggleShuffle() {
        shuffleOn = !shuffleOn
        Quickshell.execDetached(["playerctl", "shuffle", shuffleOn ? "On" : "Off"])
    }

    function cycleLoop() {
        loopMode = loopMode === "None" ? "Playlist" : (loopMode === "Playlist" ? "Track" : "None")
        Quickshell.execDetached(["playerctl", "loop", loopMode])
    }

    function cycleOutput() {
        Quickshell.execDetached([
            "sh", "-c",
            "sinks=$(pactl list short sinks 2>/dev/null | awk '{print $2}'); " +
            "cur=$(pactl get-default-sink 2>/dev/null); " +
            "next=$(printf '%s\\n' \"$sinks\" | awk -v c=\"$cur\" 'f{print;exit} $0==c{f=1}'); " +
            "[ -z \"$next\" ] && next=$(printf '%s\\n' \"$sinks\" | head -1); " +
            "[ -n \"$next\" ] && pactl set-default-sink \"$next\""
        ])
    }

    PamContext {
        id: pam
        config: "login"
        onPamMessage: { if (messageIsError) root.status = message }
        onCompleted: result => {
            if (result === PamResult.Success) { root.unlocking = true; unlockDelay.start() }
            else { root.status = "Incorrect password"; shake.restart() }
        }
    }
    Timer { id: unlockDelay; interval: Theme.ms(420); onTriggered: { root.locked = false; root.unlocking = false } }
    SequentialAnimation { id: shake; property var target: null }

    WlSessionLock {
        locked: root.locked
        WlSessionLockSurface {
            id: surf
            color: "#090a10"
            Item {
                id: stage
                anchors.fill: parent
                clip: true
                readonly property bool ambient: !!root.player
                readonly property bool showSignIn: !ambient || root.signInOpen
                readonly property bool showMusic: ambient && !root.signInOpen
                readonly property bool isDepth: showMusic && root.lockMode === "depth"
                readonly property bool isSplit: showMusic && (root.lockMode === "lyrics" || root.lockMode === "queue")
                readonly property var lyricLines: root.lyrics ? root.lyrics.lines : []
                readonly property int lyricCur: root.player && root.lyrics ? root.lyrics.indexAt(root.player.position) : -1

                // Swipe-up (mouse/touch drag or trackpad scroll) opens the dedicated Sign-In page;
                // swipe-down returns cleanly to the Now Playing music screen.
                property real dragStartY: 0
                MouseArea {
                    anchors.fill: parent
                    z: 0
                    onPressed: mouse => { stage.dragStartY = mouse.y }
                    onReleased: mouse => {
                        const dy = mouse.y - stage.dragStartY
                        if (stage.ambient && !root.signInOpen && dy < -44) root.openSignIn()
                        else if (stage.ambient && root.signInOpen && dy > 44) root.closeSignIn()
                    }
                }
                WheelHandler {
                    onWheel: event => {
                        if (!stage.ambient) return
                        const dy = event.pixelDelta.y !== 0 ? event.pixelDelta.y : event.angleDelta.y
                        if (!root.signInOpen && dy > 24) root.openSignIn()
                        else if (root.signInOpen && dy < -24) root.closeSignIn()
                    }
                }
                Keys.onPressed: event => {
                    if (!stage.ambient) return
                    if (!root.signInOpen && (event.key === Qt.Key_Up || event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter)) {
                        root.openSignIn()
                        event.accepted = true
                    } else if (root.signInOpen && (event.key === Qt.Key_Escape || event.key === Qt.Key_Down)) {
                        root.closeSignIn()
                        event.accepted = true
                    }
                }

                SystemClock { id: clk; precision: SystemClock.Minutes }
                Timer {
                    interval: 450
                    running: root.locked && root.player !== null && root.player.isPlaying
                    repeat: true
                    onTriggered: if (root.player) root.player.positionChanged()
                }

                // True edge-to-edge blurred ambient backdrop (1.36x + autoPaddingEnabled: false -> zero black edges)
                Image {
                    id: bg
                    anchors.centerIn: parent
                    width: stage.width * 1.36
                    height: stage.height * 1.36
                    fillMode: Image.PreserveAspectCrop
                    source: root.player && root.player.trackArtUrl ? root.player.trackArtUrl : "file://" + root.wallpaper
                    visible: false
                }
                MultiEffect {
                    anchors.fill: bg
                    source: bg
                    autoPaddingEnabled: false
                    blurEnabled: true
                    blur: 1.0
                    blurMax: 64
                    brightness: stage.isDepth ? -0.06 : -0.18
                    saturation: 0.45
                }

                // Mode 1 ("Full Art & Vision Clock Depth"): seamless edge-to-edge full-bleed artwork
                Image {
                    visible: stage.isDepth && root.player && root.player.trackArtUrl
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectCrop
                    source: root.player ? root.player.trackArtUrl : ""
                    opacity: 0.96
                }
                // Soft cinematic radial/side-edge ambient vignette for Full Art & Depth mode
                Rectangle {
                    visible: stage.isDepth
                    anchors.fill: parent
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0.0;  color: Qt.rgba(0.02, 0.02, 0.05, 0.36) }
                        GradientStop { position: 0.14; color: "transparent" }
                        GradientStop { position: 0.86; color: "transparent" }
                        GradientStop { position: 1.0;  color: Qt.rgba(0.02, 0.02, 0.05, 0.36) }
                    }
                }
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        GradientStop { position: 0.0; color: Qt.rgba(0.02, 0.02, 0.05, stage.isDepth ? 0.16 : 0.24) }
                        GradientStop { position: 0.52; color: Qt.rgba(0.02, 0.02, 0.05, stage.isDepth ? 0.18 : 0.42) }
                        GradientStop { position: 1.0; color: Qt.rgba(0.01, 0.01, 0.03, stage.isDepth ? 0.78 : 0.72) }
                    }
                }

                // Top hardware notch with animated lock glyph
                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 4; color: "#000000"
                    width: root.unlocking ? 150 : 104; height: 24; radius: 12
                    Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen } }
                    Icon {
                        anchors.centerIn: parent; name: "lock"; size: 15
                        color: root.unlocking ? "#30d158" : "white"
                        rotation: root.unlocking ? -18 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.ms(260); easing.type: Easing.OutBack } }
                    }
                }

                // ================================================================
                // MODE 1: FULL ART & VISION CLOCK DEPTH (04, 14, 26, 29)
                // Dramatic macOS 27 Glass Vision Clock below the notch, zero login clutter
                // ================================================================
                Column {
                    visible: stage.isDepth
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 42
                    spacing: 2
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Qt.formatDateTime(clk.date, "dddd, d MMMM")
                        color: Qt.rgba(1, 1, 1, 0.86)
                        font.family: Theme.font; font.pixelSize: 20; font.weight: Font.DemiBold
                        style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.35)
                    }
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Qt.formatDateTime(clk.date, "HH:mm")
                        color: Qt.rgba(1, 1, 1, 0.88)
                        font.family: Theme.font; font.pixelSize: 84; font.weight: Font.Bold; font.letterSpacing: -2; font.features: { "tnum": 1 }
                        style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.30)
                    }
                }

                // Centered lower translucent Liquid Glass controls overlay (Full Art & Depth)
                Column {
                    visible: stage.isDepth
                    width: Math.min(460, stage.width * 0.44)
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 64
                    spacing: 12

                    Row {
                        width: parent.width; spacing: 10
                        Column {
                            width: parent.width - 54; spacing: 2
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackTitle || "Unknown Title") : ""
                                color: "white"; font.family: Theme.font; font.pixelSize: 19; font.weight: Font.Bold
                                style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.35)
                            }
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackArtist || root.player.identity) : ""
                                color: Qt.rgba(1, 1, 1, 0.78); font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium
                            }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "star"; size: 19
                            color: root.favOn ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.72)
                            TapHandler { onTapped: root.favOn = !root.favOn }
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: "•••"; color: Qt.rgba(1, 1, 1, 0.72); font.pixelSize: 12; font.weight: Font.Bold
                        }
                    }

                    // Slim seekable scrubber
                    Column {
                        width: parent.width; spacing: 5
                        Rectangle {
                            id: depthScrub
                            width: parent.width; height: 5; radius: 2.5
                            color: Qt.rgba(1, 1, 1, 0.28)
                            Rectangle {
                                height: parent.height; radius: 2.5; color: "white"
                                width: root.player && root.player.length > 0 ? parent.width * Math.min(1, Math.max(0, root.player.position / root.player.length)) : 0
                            }
                            TapHandler {
                                onTapped: ev => {
                                    if (root.player && root.player.canSeek && root.player.length > 0)
                                        root.player.position = ev.position.x / depthScrub.width * root.player.length
                                }
                            }
                        }
                        Item {
                            width: parent.width; height: 14
                            Text { anchors.left: parent.left; text: root.fmt(root.player ? root.player.position : -1); color: Qt.rgba(1, 1, 1, 0.72); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }; font.weight: Theme.wBody;}
                            Text { anchors.right: parent.right; text: root.player && root.player.length > 0 ? "-" + root.fmt(Math.max(0, root.player.length - root.player.position)) : "--:--"; color: Qt.rgba(1, 1, 1, 0.72); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }; font.weight: Theme.wBody;}
                        }
                    }

                    // 5-button transport row
                    Item {
                        width: parent.width; height: 38
                        Icon {
                            anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                            name: "shuffle"; size: 17; color: root.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.76)
                            TapHandler { onTapped: root.toggleShuffle() }
                        }
                        Row {
                            anchors.centerIn: parent; spacing: 34
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "prev"; size: 26; color: "white"; TapHandler { onTapped: if (root.player && root.player.canGoPrevious) root.player.previous() } }
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: root.player && root.player.isPlaying ? "pause" : "play"; size: 38; color: "white"; TapHandler { onTapped: if (root.player) root.player.togglePlaying() } }
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "next"; size: 26; color: "white"; TapHandler { onTapped: if (root.player && root.player.canGoNext) root.player.next() } }
                        }
                        Icon {
                            anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                            name: "repeat"; size: 17; color: root.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.76)
                            TapHandler { onTapped: root.cycleLoop() }
                        }
                    }

                    // Inline PipeWire volume slider
                    Row {
                        width: parent.width; spacing: 10
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "volumeMute"; size: 14; color: Qt.rgba(1, 1, 1, 0.70); TapHandler { onTapped: if (root.sink) root.sink.audio.muted = !root.sink.audio.muted } }
                        Rectangle {
                            id: depthVolTrack
                            width: parent.width - 48; height: 4; radius: 2
                            anchors.verticalCenter: parent.verticalCenter
                            color: Qt.rgba(1, 1, 1, 0.28)
                            Rectangle {
                                height: parent.height; radius: 2; color: "white"
                                width: root.sink ? parent.width * Math.min(1, Math.max(0, root.sink.audio.muted ? 0 : root.sink.audio.volume)) : parent.width * 0.62
                            }
                            TapHandler {
                                onTapped: ev => {
                                    if (root.sink) {
                                        root.sink.audio.muted = false
                                        root.sink.audio.volume = Math.max(0, Math.min(1, ev.position.x / depthVolTrack.width))
                                    }
                                }
                            }
                        }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "volume"; size: 14; color: Qt.rgba(1, 1, 1, 0.70) }
                    }
                }

                // ================================================================
                // MODES 2 & 3: SPLIT-PANEL LIVE LYRICS & PLAYING NEXT (13, 18)
                // ================================================================
                Row {
                    visible: stage.isSplit
                    anchors { top: parent.top; right: parent.right; topMargin: 22; rightMargin: 32 }
                    spacing: 10
                    Text { text: Qt.formatDateTime(clk.date, "HH:mm"); color: "white"; font.family: Theme.font; font.pixelSize: 16; font.weight: Font.Bold; font.features: { "tnum": 1 } }
                    Text { text: Qt.formatDateTime(clk.date, "ddd, d MMM"); color: Qt.rgba(1, 1, 1, 0.76); font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Medium }
                }

                // Left media column (shared by Live Lyrics & Playing Next modes)
                Column {
                    visible: stage.isSplit
                    width: Math.min(392, stage.width * 0.36)
                    anchors {
                        left: parent.left
                        leftMargin: Math.max(52, stage.width * 0.065)
                        verticalCenter: parent.verticalCenter
                        verticalCenterOffset: -20
                    }
                    spacing: 14

                    Item {
                        width: Math.min(parent.width, 312)
                        height: width
                        anchors.horizontalCenter: parent.horizontalCenter
                        RectangularShadow {
                            anchors.fill: artCard
                            radius: 24; blur: 40; spread: -4; offset.y: 16
                            color: Qt.rgba(0, 0, 0, 0.52)
                        }
                        ClippingRectangle {
                            id: artCard
                            anchors.fill: parent; radius: 24; color: "#111218"
                            Rectangle {
                                anchors.fill: parent
                                gradient: Gradient {
                                    GradientStop { position: 0.0; color: "#2c5f7c" }
                                    GradientStop { position: 0.55; color: "#5f27cd" }
                                    GradientStop { position: 1.0; color: "#141824" }
                                }
                            }
                            Image { anchors.fill: parent; source: root.player ? root.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop; asynchronous: true }
                            Rectangle { anchors.fill: parent; radius: 24; color: "transparent"; border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16) }
                        }
                    }

                    Row {
                        width: parent.width; spacing: 10
                        Column {
                            width: parent.width - 54; spacing: 2
                            Text { width: parent.width; elide: Text.ElideRight; text: root.player ? (root.player.trackTitle || "Unknown Title") : ""; color: "white"; font.family: Theme.font; font.pixelSize: 20; font.weight: Font.Bold }
                            Text { width: parent.width; elide: Text.ElideRight; text: root.player ? (root.player.trackArtist || root.player.identity) : ""; color: Qt.rgba(1, 1, 1, 0.70); font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium }
                        }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "star"; size: 19; color: root.favOn ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.62); TapHandler { onTapped: root.favOn = !root.favOn } }
                        Text { anchors.verticalCenter: parent.verticalCenter; text: "•••"; color: Qt.rgba(1, 1, 1, 0.62); font.pixelSize: 12; font.weight: Font.Bold }
                    }

                    Column {
                        width: parent.width; spacing: 6
                        Rectangle {
                            id: lockScrub
                            width: parent.width; height: 6; radius: 3; color: Qt.rgba(1, 1, 1, 0.24)
                            Rectangle {
                                height: parent.height; radius: 3; color: "white"
                                width: root.player && root.player.length > 0 ? parent.width * Math.min(1, Math.max(0, root.player.position / root.player.length)) : 0
                            }
                            TapHandler {
                                onTapped: ev => {
                                    if (root.player && root.player.canSeek && root.player.length > 0)
                                        root.player.position = ev.position.x / lockScrub.width * root.player.length
                                }
                            }
                        }
                        Item {
                            width: parent.width; height: 14
                            Text { anchors.left: parent.left; text: root.fmt(root.player ? root.player.position : -1); color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }; font.weight: Theme.wBody;}
                            Text { anchors.right: parent.right; text: root.player && root.player.length > 0 ? "-" + root.fmt(Math.max(0, root.player.length - root.player.position)) : "--:--"; color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }; font.weight: Theme.wBody;}
                        }
                    }

                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter; spacing: 30
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "shuffle"; size: 18; color: root.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.64); TapHandler { onTapped: root.toggleShuffle() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "prev"; size: 25; color: "white"; TapHandler { onTapped: if (root.player && root.player.canGoPrevious) root.player.previous() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: root.player && root.player.isPlaying ? "pause" : "play"; size: 36; color: "white"; TapHandler { onTapped: if (root.player) root.player.togglePlaying() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "next"; size: 25; color: "white"; TapHandler { onTapped: if (root.player && root.player.canGoNext) root.player.next() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "repeat"; size: 18; color: root.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.64); TapHandler { onTapped: root.cycleLoop() } }
                    }

                    Row {
                        width: parent.width; spacing: 10
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "volumeMute"; size: 14; color: Qt.rgba(1, 1, 1, 0.62); TapHandler { onTapped: if (root.sink) root.sink.audio.muted = !root.sink.audio.muted } }
                        Rectangle {
                            id: volTrack
                            width: parent.width - 48; height: 5; radius: 2.5
                            anchors.verticalCenter: parent.verticalCenter
                            color: Qt.rgba(1, 1, 1, 0.24)
                            Rectangle {
                                height: parent.height; radius: 2.5; color: "white"
                                width: root.sink ? parent.width * Math.min(1, Math.max(0, root.sink.audio.muted ? 0 : root.sink.audio.volume)) : parent.width * 0.6
                            }
                            TapHandler {
                                onTapped: ev => {
                                    if (root.sink) {
                                        root.sink.audio.muted = false
                                        root.sink.audio.volume = Math.max(0, Math.min(1, ev.position.x / volTrack.width))
                                    }
                                }
                            }
                        }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "volume"; size: 14; color: Qt.rgba(1, 1, 1, 0.62) }
                    }
                }

                // Right column: Mode 2 (Live Lyrics) OR Mode 3 (Playing Next Queue — exact match to 13_droppy_lock_split_playing_next.jpg!)
                Item {
                    visible: stage.isSplit
                    anchors {
                        left: parent.horizontalCenter
                        leftMargin: 24
                        right: parent.right
                        rightMargin: Math.max(52, stage.width * 0.065)
                        top: parent.top
                        topMargin: 72
                        bottom: parent.bottom
                        bottomMargin: 76
                    }
                    clip: true

                    // MODE 2: LIVE SYNCED LYRICS
                    Text {
                        visible: root.lockMode === "lyrics" && stage.lyricLines.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        text: root.lyrics && root.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                        color: Qt.rgba(1, 1, 1, 0.44)
                        font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold
                    }
                    Column {
                        visible: root.lockMode === "lyrics" && stage.lyricLines.length > 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        spacing: 20
                        Repeater {
                            model: [-1, 0, 1, 2, 3, 4]
                            Text {
                                required property int modelData
                                readonly property int idx: (stage.lyricCur >= 0 ? stage.lyricCur : 0) + modelData
                                visible: idx >= 0 && idx < stage.lyricLines.length
                                width: parent.width
                                wrapMode: Text.WordWrap
                                text: idx >= 0 && idx < stage.lyricLines.length ? stage.lyricLines[idx].line : ""
                                color: modelData === 0 ? "white"
                                     : modelData === -1 ? Qt.rgba(1, 1, 1, 0.36)
                                     : modelData === 1 ? Qt.rgba(1, 1, 1, 0.48)
                                     : modelData === 2 ? Qt.rgba(1, 1, 1, 0.34)
                                     : modelData === 3 ? Qt.rgba(1, 1, 1, 0.24)
                                     : Qt.rgba(1, 1, 1, 0.16)
                                font.family: Theme.font
                                font.pixelSize: modelData === 0 ? 31 : 23
                                font.weight: modelData === 0 ? Font.Bold : Font.DemiBold
                            }
                        }
                    }

                    // MODE 3: PLAYING NEXT QUEUE (13_droppy_lock_split_playing_next.jpg)
                    Column {
                        visible: root.lockMode === "queue"
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        spacing: 16

                        Item {
                            width: parent.width; height: 48
                            Column {
                                anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; spacing: 2
                                Text { text: "Playing Next"; color: "white"; font.family: Theme.font; font.pixelSize: 24; font.weight: Font.Bold }
                                Text { text: "Continue Playing"; color: Qt.rgba(1, 1, 1, 0.56); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
                            }
                            Row {
                                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; spacing: 8
                                Rectangle {
                                    width: 34; height: 34; radius: 17
                                    color: root.shuffleOn ? "white" : Qt.rgba(1, 1, 1, 0.14)
                                    Icon { anchors.centerIn: parent; name: "shuffle"; size: 15; color: root.shuffleOn ? "#16161a" : "white" }
                                    TapHandler { onTapped: root.toggleShuffle() }
                                }
                                Rectangle {
                                    width: 34; height: 34; radius: 17
                                    color: root.loopMode !== "None" ? "white" : Qt.rgba(1, 1, 1, 0.14)
                                    Icon { anchors.centerIn: parent; name: "repeat"; size: 15; color: root.loopMode !== "None" ? "#16161a" : "white" }
                                    TapHandler { onTapped: root.cycleLoop() }
                                }
                            }
                        }

                        Column {
                            width: parent.width; spacing: 10
                            Repeater {
                                model: root.queueItems
                                Rectangle {
                                    required property var modelData
                                    required property int index
                                    width: parent.width; height: 60; radius: 14
                                    color: qh.hovered ? Qt.rgba(1, 1, 1, 0.14) : Qt.rgba(1, 1, 1, 0.06)
                                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.08)
                                    Row {
                                        anchors { left: parent.left; leftMargin: 10; right: durTxt.left; rightMargin: 10; verticalCenter: parent.verticalCenter }
                                        spacing: 12
                                        Rectangle {
                                            width: 44; height: 44; radius: 10
                                            gradient: Gradient {
                                                GradientStop { position: 0; color: modelData.c1 || "#2563eb" }
                                                GradientStop { position: 1; color: modelData.c2 || "#06b6d4" }
                                            }
                                            Icon { anchors.centerIn: parent; name: "music"; size: 18; color: Qt.rgba(1, 1, 1, 0.90) }
                                        }
                                        Column {
                                            anchors.verticalCenter: parent.verticalCenter
                                            width: parent.width - 56; spacing: 2
                                            Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
                                            Text { width: parent.width; elide: Text.ElideRight; text: modelData.artist; color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wBody;}
                                        }
                                    }
                                    Text {
                                        id: durTxt
                                        anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
                                        text: modelData.duration || "3:30"
                                        color: Qt.rgba(1, 1, 1, 0.56); font.family: Theme.font; font.pixelSize: 12; font.features: { "tnum": 1 }
                                        font.weight: Theme.wBody
                                    }
                                    HoverHandler { id: qh }
                                    TapHandler { onTapped: if (root.shell) root.shell.playQueueItem(index) }
                                }
                            }
                        }
                    }
                }

                // Bottom-left circular frosted Output Device button
                Rectangle {
                    visible: stage.showMusic
                    anchors { left: parent.left; bottom: parent.bottom; leftMargin: 32; bottomMargin: 24 }
                    width: 38; height: 38; radius: 19
                    color: Qt.rgba(1, 1, 1, 0.14)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.22)
                    Icon { anchors.centerIn: parent; name: "desktop"; size: 17; color: "white" }
                    TapHandler { onTapped: root.cycleOutput() }
                }

                // Bottom-center Swipe-Up Home Bar (replaces cluttered password box while music plays)
                Column {
                    visible: stage.showMusic
                    anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 16 }
                    spacing: 6
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: "Swipe up to sign in"
                        color: Qt.rgba(1, 1, 1, swipeHov.hovered ? 0.85 : 0.46)
                        font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium
                        Behavior on color { ColorAnimation { duration: Theme.ms(140) } }
                    }
                    Rectangle {
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: swipeHov.hovered ? 156 : 136; height: 5; radius: 2.5
                        color: Qt.rgba(1, 1, 1, swipeHov.hovered ? 0.88 : 0.52)
                        Behavior on width { NumberAnimation { duration: Theme.ms(160); easing.type: Easing.OutCubic } }
                    }
                    HoverHandler { id: swipeHov }
                    TapHandler { onTapped: root.openSignIn() }
                }

                // Bottom-right 3-Mode Droppy Segmented Pill (Full Art & Depth | Live Lyrics | Playing Next)
                Rectangle {
                    visible: stage.showMusic
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: 32; bottomMargin: 24 }
                    width: 124; height: 38; radius: 19
                    color: Qt.rgba(1, 1, 1, 0.14)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.22)
                    Row {
                        anchors.centerIn: parent; spacing: 4
                        Rectangle {
                            width: 36; height: 30; radius: 15
                            color: root.lockMode === "depth" ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "depth"; size: 15; color: root.lockMode === "depth" ? "#16161a" : "white" }
                            TapHandler { onTapped: root.setMode("depth") }
                        }
                        Rectangle {
                            width: 36; height: 30; radius: 15
                            color: root.lockMode === "lyrics" ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "lyrics"; size: 15; color: root.lockMode === "lyrics" ? "#16161a" : "white" }
                            TapHandler { onTapped: root.setMode("lyrics") }
                        }
                        Rectangle {
                            width: 36; height: 30; radius: 15
                            color: root.lockMode === "queue" ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "queue"; size: 15; color: root.lockMode === "queue" ? "#16161a" : "white" }
                            TapHandler { onTapped: root.setMode("queue") }
                        }
                    }
                }

                // ================================================================
                // DEDICATED SIGN-IN PAGE (shown on swipe-up during music, or when idle)
                // ================================================================
                Item {
                    id: signInPage
                    anchors.fill: parent
                    z: 20
                    visible: opacity > 0.01
                    opacity: stage.showSignIn ? 1.0 : 0.0
                    y: stage.showSignIn ? 0 : 48
                    Behavior on opacity { NumberAnimation { duration: Theme.ms(240); easing.type: Easing.OutCubic } }
                    Behavior on y { NumberAnimation { duration: Theme.ms(280); easing.type: Easing.OutCubic } }
                    onVisibleChanged: if (visible) pw.forceActiveFocus()

                    // Soft dark frosted veil over the blurred ambient backdrop
                    Rectangle {
                        anchors.fill: parent
                        color: Qt.rgba(0.03, 0.035, 0.055, stage.ambient ? 0.56 : 0.22)
                    }

                    // Top-center subtle pill to return to Now Playing when music is active
                    Rectangle {
                        visible: stage.ambient
                        anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 42 }
                        width: backRow.implicitWidth + 24; height: 30; radius: 15
                        color: Qt.rgba(1, 1, 1, backHov.hovered ? 0.22 : 0.12)
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.24)
                        Row {
                            id: backRow
                            anchors.centerIn: parent; spacing: 6
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "down"; size: 12; color: "white" }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: root.player ? ("Now Playing — " + (root.player.trackTitle || "Music")) : "Now Playing"
                                color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                            }
                        }
                        HoverHandler { id: backHov }
                        TapHandler { onTapped: root.closeSignIn() }
                    }

                    // Large macOS 27 Lock Clock
                    Column {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: parent.height * 0.14
                        spacing: 0
                        Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "dddd, d MMMM"); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold }
                        Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "H:mm"); color: "white"; font.family: Theme.font; font.pixelSize: 112; font.weight: Font.Bold; font.features: { "tnum": 1 } }
                    }

                    // Centered PAM Password Unlock
                    Column {
                        id: unlockArea
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.bottom: parent.bottom
                        anchors.bottomMargin: parent.height * 0.09
                        spacing: 10

                        Rectangle {
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: 72; height: 72; radius: 36
                            gradient: Gradient {
                                GradientStop { position: 0; color: "#8e8e93" }
                                GradientStop { position: 1; color: "#5a5a5f" }
                            }
                            border.width: 1.2; border.color: Qt.rgba(1, 1, 1, 0.36)
                            Text { anchors.centerIn: parent; text: (Quickshell.env("USER") || "?").charAt(0).toUpperCase(); color: "white"; font.family: Theme.font; font.pixelSize: 30; font.weight: Font.DemiBold }
                        }
                        Text {
                            anchors.horizontalCenter: parent.horizontalCenter
                            bottomPadding: 2
                            text: { const u = Quickshell.env("USER") || "User"; return u.charAt(0).toUpperCase() + u.slice(1) }
                            color: "white"; font.family: Theme.font; font.pixelSize: 16; font.weight: Font.DemiBold
                        }

                        // Cupertino Liquid Glass Password Capsule
                        Glass {
                            id: pwBox
                            kind: "module"
                            r: 18
                            shadow: false
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: 248; height: 36
                            TextField {
                                id: pw
                                anchors.fill: parent; anchors.leftMargin: 16; anchors.rightMargin: 16
                                echoMode: TextInput.Password
                                placeholderText: ""
                                color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.6)
                                font.family: Theme.font; font.pixelSize: 13
                                horizontalAlignment: TextInput.AlignHCenter
                                background: null
                                focus: stage.showSignIn
                                enabled: !pam.active
                                onAccepted: { pam.start() }
                                Keys.onEscapePressed: if (stage.ambient) root.closeSignIn()
                            }
                            Text {
                                anchors.centerIn: parent
                                visible: pw.text === "" && !pam.active
                                text: "Enter Password"
                                color: Qt.rgba(1, 1, 1, 0.66); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
                            }
                            Connections {
                                target: pam
                                function onResponseRequiredChanged() { if (pam.responseRequired) { pam.respond(pw.text); pw.text = "" } }
                            }
                        }

                        Text {
                            visible: root.status !== ""
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: root.status; color: "#ff8f8f"; font.family: Theme.font; font.pixelSize: 12
                            font.weight: Theme.wBody
                        }

                        Row {
                            anchors.horizontalCenter: parent.horizontalCenter
                            spacing: 6
                            visible: UPower.displayDevice.isLaptopBattery
                            Icon { name: UPower.displayDevice.state === UPowerDeviceState.Charging ? "charging" : "battery"; size: 15; color: "white" }
                            Text { text: Math.round(UPower.displayDevice.percentage * (UPower.displayDevice.percentage > 1 ? 1 : 100)) + "%"; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody;}
                        }
                        Row {
                            anchors.horizontalCenter: parent.horizontalCenter
                            topPadding: 8
                            spacing: 16
                            Repeater {
                                model: [
                                    { label: "Sleep", cmd: ["systemctl", "suspend"] },
                                    { label: "Restart", cmd: ["systemctl", "reboot"] },
                                    { label: "Shut Down", cmd: ["systemctl", "poweroff"] }
                                ]
                                Glass {
                                    required property var modelData
                                    kind: "module"; r: 14; shadow: false
                                    width: 78; height: 28
                                    Text {
                                        anchors.centerIn: parent
                                        text: modelData.label; color: Qt.rgba(1, 1, 1, 0.88)
                                        font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody
                                    }
                                    TapHandler { onTapped: Quickshell.execDetached(modelData.cmd) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
