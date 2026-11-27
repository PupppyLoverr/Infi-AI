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

// Exact getdroppy.app Fullscreen Ambient Lyrics Lock Screen (ext-session-lock + PAM):
// - When media is active: full-bleed edge-to-edge blurred album art (zero black
//   border bars), top hardware lock notch, top-right compact clock/date, large
//   left album artwork + title/artist/star + live seekable scrubber + 5-button
//   transport + inline PipeWire volume slider, large right-column scrolling
//   live synced lyrics, bottom-left Output Device pill, bottom-right Lyrics/Queue
//   segmented pill, and sleek bottom PAM unlock capsule.
// - When idle: clean centered macOS 27 Lock Screen.
Scope {
    id: root
    property var lyrics
    property bool locked: false
    property string wallpaper: Theme.home + "/.config/solyra/wallpaper.png"
    property string status: ""
    property bool unlocking: false
    property bool shuffleOn: false
    property string loopMode: "None"
    property bool favOn: false
    property bool showLyricsPane: true

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
                readonly property var lyricLines: root.lyrics ? root.lyrics.lines : []
                readonly property int lyricCur: root.player && root.lyrics ? root.lyrics.indexAt(root.player.position) : -1

                SystemClock { id: clk; precision: SystemClock.Minutes }
                Timer {
                    interval: 450
                    running: root.locked && root.player !== null && root.player.isPlaying
                    repeat: true
                    onTriggered: if (root.player) root.player.positionChanged()
                }

                // True edge-to-edge blurred ambient backdrop: oversized 1.36x +
                // autoPaddingEnabled: false so MultiEffect never introduces black frame edges.
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
                    brightness: -0.18
                    saturation: 0.42
                }
                // Subtle cinematic radial/vertical ambient vignette
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        GradientStop { position: 0.0; color: Qt.rgba(0.03, 0.03, 0.06, 0.24) }
                        GradientStop { position: 0.6; color: Qt.rgba(0.03, 0.03, 0.06, 0.42) }
                        GradientStop { position: 1.0; color: Qt.rgba(0.01, 0.01, 0.03, 0.72) }
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

                // ------------------------------------------------ DROPPY FULLSCREEN AMBIENT MEDIA MODE
                // Top-right crisp clock & date ("08:21  Sun, 22 Nov")
                Row {
                    visible: stage.ambient
                    anchors { top: parent.top; right: parent.right; topMargin: 22; rightMargin: 32 }
                    spacing: 10
                    Text {
                        text: Qt.formatDateTime(clk.date, "HH:mm")
                        color: "white"; font.family: Theme.font; font.pixelSize: 16; font.weight: Font.Bold; font.features: { "tnum": 1 }
                    }
                    Text {
                        text: Qt.formatDateTime(clk.date, "ddd, d MMM")
                        color: Qt.rgba(1, 1, 1, 0.76); font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Medium
                    }
                }

                // Left media column: large full-bleed album artwork + metadata + scrubber + 5-button transport + volume
                Column {
                    visible: stage.ambient
                    width: Math.min(400, stage.width * 0.37)
                    anchors {
                        left: parent.left
                        leftMargin: Math.max(52, stage.width * 0.065)
                        verticalCenter: parent.verticalCenter
                        verticalCenterOffset: -20
                    }
                    spacing: 14

                    Item {
                        width: Math.min(parent.width, 320)
                        height: width
                        anchors.horizontalCenter: parent.horizontalCenter
                        RectangularShadow {
                            anchors.fill: artCard
                            radius: 24; blur: 40; spread: -4; offset.y: 16
                            color: Qt.rgba(0, 0, 0, 0.52)
                        }
                        ClippingRectangle {
                            id: artCard
                            anchors.fill: parent
                            radius: 24
                            color: "#111218"
                            Rectangle {
                                anchors.fill: parent
                                gradient: Gradient {
                                    GradientStop { position: 0.0; color: "#2c5f7c" }
                                    GradientStop { position: 0.55; color: "#5f27cd" }
                                    GradientStop { position: 1.0; color: "#141824" }
                                }
                            }
                            Image {
                                anchors.fill: parent
                                source: root.player ? root.player.trackArtUrl : ""
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                            }
                            Rectangle {
                                anchors.fill: parent; radius: 24; color: "transparent"
                                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
                            }
                        }
                    }

                    // Track title, artist, favorite star & "..."
                    Row {
                        width: parent.width
                        spacing: 10
                        Column {
                            width: parent.width - 54
                            spacing: 2
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackTitle || "Unknown Title") : ""
                                color: "white"; font.family: Theme.font; font.pixelSize: 20; font.weight: Font.Bold
                            }
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackArtist || root.player.identity) : ""
                                color: Qt.rgba(1, 1, 1, 0.70); font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium
                            }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "star"; size: 19
                            color: root.favOn ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.62)
                            TapHandler { onTapped: root.favOn = !root.favOn }
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: "•••"; color: Qt.rgba(1, 1, 1, 0.62); font.pixelSize: 12; font.weight: Font.Bold
                        }
                    }

                    // Seekable scrubber with elapsed / remaining
                    Column {
                        width: parent.width
                        spacing: 6
                        Rectangle {
                            id: lockScrub
                            width: parent.width; height: 6; radius: 3
                            color: Qt.rgba(1, 1, 1, 0.24)
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
                            Text {
                                anchors.left: parent.left
                                text: root.fmt(root.player ? root.player.position : -1)
                                color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }
                            }
                            Text {
                                anchors.right: parent.right
                                text: root.player && root.player.length > 0 ? "-" + root.fmt(Math.max(0, root.player.length - root.player.position)) : "--:--"
                                color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }
                            }
                        }
                    }

                    // 5-button transport row: Shuffle, Prev, Play/Pause, Next, Repeat
                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 30
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "shuffle"; size: 18
                            color: root.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.64)
                            TapHandler { onTapped: root.toggleShuffle() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "prev"; size: 25; color: "white"
                            TapHandler { onTapped: if (root.player && root.player.canGoPrevious) root.player.previous() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: root.player && root.player.isPlaying ? "pause" : "play"
                            size: 36; color: "white"
                            TapHandler { onTapped: if (root.player) root.player.togglePlaying() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "next"; size: 25; color: "white"
                            TapHandler { onTapped: if (root.player && root.player.canGoNext) root.player.next() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "repeat"; size: 18
                            color: root.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.64)
                            TapHandler { onTapped: root.cycleLoop() }
                        }
                    }

                    // Inline interactive PipeWire volume slider
                    Row {
                        width: parent.width
                        spacing: 10
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "volumeMute"; size: 14; color: Qt.rgba(1, 1, 1, 0.62)
                            TapHandler { onTapped: if (root.sink) root.sink.audio.muted = !root.sink.audio.muted }
                        }
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
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "volume"; size: 14; color: Qt.rgba(1, 1, 1, 0.62)
                        }
                    }
                }

                // Right column: Large scrolling Live Synced Lyrics (exact Droppy spacing & falloff)
                Item {
                    visible: stage.ambient
                    anchors {
                        left: parent.horizontalCenter
                        leftMargin: 20
                        right: parent.right
                        rightMargin: Math.max(52, stage.width * 0.065)
                        top: parent.top
                        topMargin: 72
                        bottom: unlockArea.top
                        bottomMargin: 24
                    }
                    clip: true

                    Text {
                        visible: root.showLyricsPane && stage.lyricLines.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        text: root.lyrics && root.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                        color: Qt.rgba(1, 1, 1, 0.44)
                        font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold
                    }

                    Column {
                        visible: root.showLyricsPane && stage.lyricLines.length > 0
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

                    Column {
                        visible: !root.showLyricsPane
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        spacing: 12
                        Text { text: "NOW PLAYING"; color: Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold }
                        Text { width: parent.width; wrapMode: Text.WordWrap; text: root.player ? (root.player.trackTitle || "Unknown") : ""; color: "white"; font.family: Theme.font; font.pixelSize: 28; font.weight: Font.Bold }
                        Text { width: parent.width; wrapMode: Text.WordWrap; text: root.player ? (root.player.trackArtist || "") : ""; color: Qt.rgba(1, 1, 1, 0.72); font.family: Theme.font; font.pixelSize: 18 }
                        Text { text: root.player && root.player.identity ? ("Source · " + root.player.identity) : ""; color: Qt.rgba(1, 1, 1, 0.48); font.family: Theme.font; font.pixelSize: 13 }
                    }
                }

                // Bottom-left circular frosted Output Device button (exact Droppy match)
                Rectangle {
                    visible: stage.ambient
                    anchors { left: parent.left; bottom: parent.bottom; leftMargin: 32; bottomMargin: 24 }
                    width: 38; height: 38; radius: 19
                    color: Qt.rgba(1, 1, 1, 0.14)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.22)
                    Icon { anchors.centerIn: parent; name: "desktop"; size: 17; color: "white" }
                }

                // Bottom-right Lyrics / Queue segmented pill (exact Droppy match)
                Rectangle {
                    visible: stage.ambient
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: 32; bottomMargin: 24 }
                    width: 86; height: 38; radius: 19
                    color: Qt.rgba(1, 1, 1, 0.14)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.22)
                    Row {
                        anchors.centerIn: parent; spacing: 4
                        Rectangle {
                            width: 36; height: 30; radius: 15
                            color: root.showLyricsPane ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "lyrics"; size: 15; color: root.showLyricsPane ? "#16161a" : "white" }
                            TapHandler { onTapped: root.showLyricsPane = true }
                        }
                        Rectangle {
                            width: 36; height: 30; radius: 15
                            color: !root.showLyricsPane ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "queue"; size: 15; color: !root.showLyricsPane ? "#16161a" : "white" }
                            TapHandler { onTapped: root.showLyricsPane = false }
                        }
                    }
                }

                // ------------------------------------------------ CLASSIC IDLE MODE (no media)
                Column {
                    visible: !stage.ambient
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: parent.height * 0.14
                    spacing: 0
                    Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "dddd, d MMMM"); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold }
                    Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "H:mm"); color: "white"; font.family: Theme.font; font.pixelSize: 112; font.weight: Font.Bold }
                }

                // ------------------------------------------------ SHARED PAM PASSWORD UNLOCK
                Column {
                    id: unlockArea
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: stage.ambient ? 20 : parent.height * 0.08
                    spacing: stage.ambient ? 6 : 8

                    Row {
                        visible: stage.ambient
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 9
                        Rectangle {
                            width: 30; height: 30; radius: 15
                            anchors.verticalCenter: parent.verticalCenter
                            gradient: Gradient {
                                GradientStop { position: 0; color: "#8e8e93" }
                                GradientStop { position: 1; color: "#5a5a5f" }
                            }
                            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.3)
                            Text { anchors.centerIn: parent; text: (Quickshell.env("USER") || "?").charAt(0).toUpperCase(); color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold }
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: { const u = Quickshell.env("USER") || "User"; return u.charAt(0).toUpperCase() + u.slice(1) }
                            color: Qt.rgba(1, 1, 1, 0.9); font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold
                        }
                    }

                    Rectangle {
                        visible: !stage.ambient
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 64; height: 64; radius: 32
                        gradient: Gradient {
                            GradientStop { position: 0; color: "#8e8e93" }
                            GradientStop { position: 1; color: "#5a5a5f" }
                        }
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.3)
                        Text { anchors.centerIn: parent; text: (Quickshell.env("USER") || "?").charAt(0).toUpperCase(); color: "white"; font.family: Theme.font; font.pixelSize: 28; font.weight: Font.DemiBold }
                    }
                    Text {
                        visible: !stage.ambient
                        anchors.horizontalCenter: parent.horizontalCenter
                        bottomPadding: 4
                        text: { const u = Quickshell.env("USER") || "User"; return u.charAt(0).toUpperCase() + u.slice(1) }
                        color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.DemiBold
                    }

                    Rectangle {
                        id: pwBox
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 236; height: 34; radius: 17
                        color: Qt.rgba(1, 1, 1, 0.14)
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.24)
                        TextField {
                            id: pw
                            anchors.fill: parent; anchors.leftMargin: 16; anchors.rightMargin: 16
                            echoMode: TextInput.Password
                            placeholderText: ""
                            color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.6)
                            font.family: Theme.font; font.pixelSize: 13
                            horizontalAlignment: TextInput.AlignHCenter
                            background: null
                            focus: true
                            enabled: !pam.active
                            onAccepted: { pam.start() }
                            Component.onCompleted: forceActiveFocus()
                        }
                        Text {
                            anchors.centerIn: parent
                            visible: pw.text === "" && !pam.active
                            text: "Enter Password"
                            color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 13
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
                    }

                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 6
                        visible: UPower.displayDevice.isLaptopBattery
                        Icon { name: UPower.displayDevice.state === UPowerDeviceState.Charging ? "charging" : "battery"; size: 15; color: "white" }
                        Text { text: Math.round(UPower.displayDevice.percentage * (UPower.displayDevice.percentage > 1 ? 1 : 100)) + "%"; color: "white"; font.family: Theme.font; font.pixelSize: 12 }
                    }
                }
            }
        }
    }
}
