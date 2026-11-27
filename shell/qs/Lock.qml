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

// Droppy Lock Screen (ext-session-lock + PAM):
// - When media is active: Fullscreen Split-Panel Ambient Lyrics Mode
//   (large left album art + title/artist + seekable scrubber + 5-button
//    transport + inline PipeWire volume slider, top-right compact clock/date,
//    large scrolling multi-line synced lyrics on the right, and bottom
//    unlock bar).
// - When idle (no media): Clean centered macOS Lock Screen with large clock,
//   user avatar, password capsule, and battery status.
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
            color: "black"
            Item {
                id: stage
                anchors.fill: parent
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

                // Full-bleed blurred album art (or wallpaper) backdrop
                Image {
                    id: bg
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectCrop
                    source: root.player && root.player.trackArtUrl ? root.player.trackArtUrl : "file://" + root.wallpaper
                    visible: false
                }
                MultiEffect {
                    anchors.fill: parent; source: bg
                    blurEnabled: true; blur: 1.0; blurMax: 64; brightness: -0.22; saturation: 0.35
                }
                // Warm ambient vignette matching Droppy fullscreen media mode
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        GradientStop { position: 0.0; color: Qt.rgba(0.05, 0.04, 0.08, 0.36) }
                        GradientStop { position: 0.65; color: Qt.rgba(0.04, 0.03, 0.07, 0.52) }
                        GradientStop { position: 1.0; color: Qt.rgba(0.02, 0.02, 0.04, 0.78) }
                    }
                }

                // Hardware top notch with animated lock glyph
                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 4; color: "black"
                    width: root.unlocking ? 150 : 104; height: 24; radius: 12
                    Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen } }
                    Icon {
                        anchors.centerIn: parent; name: "lock"; size: 15
                        color: root.unlocking ? "#30d158" : "white"
                        rotation: root.unlocking ? -18 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.ms(260); easing.type: Easing.OutBack } }
                    }
                }

                // ------------------------------------------------ AMBIENT MEDIA MODE (Droppy split-panel)
                // Top-right compact clock & date ("08:21  Sun, 22 Nov")
                Row {
                    visible: stage.ambient
                    anchors { top: parent.top; right: parent.right; topMargin: 24; rightMargin: 32 }
                    spacing: 10
                    Text {
                        text: Qt.formatDateTime(clk.date, "HH:mm")
                        color: "white"; font.family: Theme.font; font.pixelSize: 16; font.weight: Font.Bold; font.features: { "tnum": 1 }
                    }
                    Text {
                        text: Qt.formatDateTime(clk.date, "ddd, d MMM")
                        color: Qt.rgba(1, 1, 1, 0.72); font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Medium
                    }
                }

                // Left media column: large album art + metadata + scrubber + 5-button transport + volume slider
                Column {
                    visible: stage.ambient
                    width: Math.min(390, stage.width * 0.36)
                    anchors {
                        left: parent.left
                        leftMargin: Math.max(48, stage.width * 0.065)
                        verticalCenter: parent.verticalCenter
                        verticalCenterOffset: -24
                    }
                    spacing: 14

                    // Large rounded album artwork with soft shadow
                    Item {
                        width: Math.min(parent.width, 300)
                        height: width
                        anchors.horizontalCenter: parent.horizontalCenter
                        RectangularShadow {
                            anchors.fill: artCard
                            radius: 22; blur: 36; spread: -4; offset.y: 14
                            color: Qt.rgba(0, 0, 0, 0.50)
                        }
                        Rectangle {
                            id: artCard
                            anchors.fill: parent; radius: 22; clip: true
                            gradient: Gradient {
                                GradientStop { position: 0.0; color: "#2c5f7c" }
                                GradientStop { position: 0.55; color: "#3d7a8a" }
                                GradientStop { position: 1.0; color: "#143844" }
                            }
                            Image {
                                anchors.fill: parent
                                source: root.player ? root.player.trackArtUrl : ""
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                            }
                            Rectangle {
                                anchors.fill: parent; radius: 22; color: "transparent"
                                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
                            }
                        }
                    }

                    // Track title, artist, favorite star
                    Row {
                        width: parent.width
                        spacing: 10
                        Column {
                            width: parent.width - 36
                            spacing: 2
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackTitle || "Unknown Title") : ""
                                color: "white"; font.family: Theme.font; font.pixelSize: 20; font.weight: Font.Bold
                            }
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: root.player ? (root.player.trackArtist || root.player.identity) : ""
                                color: Qt.rgba(1, 1, 1, 0.68); font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium
                            }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "star"; size: 20
                            color: root.favOn ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.55)
                            TapHandler { onTapped: root.favOn = !root.favOn }
                        }
                    }

                    // Seekable scrubber with elapsed / remaining timestamps
                    Column {
                        width: parent.width
                        spacing: 6
                        Rectangle {
                            id: lockScrub
                            width: parent.width; height: 6; radius: 3
                            color: Qt.rgba(1, 1, 1, 0.22)
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
                                color: Qt.rgba(1, 1, 1, 0.58); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }
                            }
                            Text {
                                anchors.right: parent.right
                                text: root.player && root.player.length > 0 ? "-" + root.fmt(Math.max(0, root.player.length - root.player.position)) : "--:--"
                                color: Qt.rgba(1, 1, 1, 0.58); font.family: Theme.font; font.pixelSize: 11; font.features: { "tnum": 1 }
                            }
                        }
                    }

                    // 5-button transport row: Shuffle, Prev, Play/Pause, Next, Repeat
                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 28
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "shuffle"; size: 18
                            color: root.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.58)
                            TapHandler { onTapped: root.toggleShuffle() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "prev"; size: 24; color: "white"
                            TapHandler { onTapped: if (root.player && root.player.canGoPrevious) root.player.previous() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: root.player && root.player.isPlaying ? "pause" : "play"
                            size: 34; color: "white"
                            TapHandler { onTapped: if (root.player) root.player.togglePlaying() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "next"; size: 24; color: "white"
                            TapHandler { onTapped: if (root.player && root.player.canGoNext) root.player.next() }
                        }
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "repeat"; size: 18
                            color: root.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.58)
                            TapHandler { onTapped: root.cycleLoop() }
                        }
                    }

                    // Inline interactive PipeWire volume slider
                    Row {
                        width: parent.width
                        spacing: 10
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            name: "volumeMute"; size: 14; color: Qt.rgba(1, 1, 1, 0.58)
                            TapHandler { onTapped: if (root.sink) root.sink.audio.muted = !root.sink.audio.muted }
                        }
                        Rectangle {
                            id: volTrack
                            width: parent.width - 48; height: 5; radius: 2.5
                            anchors.verticalCenter: parent.verticalCenter
                            color: Qt.rgba(1, 1, 1, 0.22)
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
                            name: "volume"; size: 14; color: Qt.rgba(1, 1, 1, 0.58)
                        }
                    }
                }

                // Right column: Large scrolling Live Synced Lyrics (or track details)
                Item {
                    visible: stage.ambient
                    anchors {
                        left: parent.horizontalCenter
                        leftMargin: 16
                        right: parent.right
                        rightMargin: Math.max(48, stage.width * 0.06)
                        top: parent.top
                        topMargin: 76
                        bottom: unlockArea.top
                        bottomMargin: 28
                    }
                    clip: true

                    Text {
                        visible: root.showLyricsPane && stage.lyricLines.length === 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        text: root.lyrics && root.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                        color: Qt.rgba(1, 1, 1, 0.42)
                        font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold
                    }

                    Column {
                        visible: root.showLyricsPane && stage.lyricLines.length > 0
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        spacing: 18
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
                                     : modelData === -1 ? Qt.rgba(1, 1, 1, 0.34)
                                     : modelData === 1 ? Qt.rgba(1, 1, 1, 0.48)
                                     : modelData === 2 ? Qt.rgba(1, 1, 1, 0.34)
                                     : modelData === 3 ? Qt.rgba(1, 1, 1, 0.24)
                                     : Qt.rgba(1, 1, 1, 0.16)
                                font.family: Theme.font
                                font.pixelSize: modelData === 0 ? 30 : 22
                                font.weight: modelData === 0 ? Font.Bold : Font.DemiBold
                            }
                        }
                    }

                    // Track details view when Queue pill is selected
                    Column {
                        visible: !root.showLyricsPane
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        spacing: 12
                        Text { text: "NOW PLAYING"; color: Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold }
                        Text { width: parent.width; wrapMode: Text.WordWrap; text: root.player ? (root.player.trackTitle || "Unknown") : ""; color: "white"; font.family: Theme.font; font.pixelSize: 28; font.weight: Font.Bold }
                        Text { width: parent.width; wrapMode: Text.WordWrap; text: root.player ? (root.player.trackArtist || "") : ""; color: Qt.rgba(1, 1, 1, 0.72); font.family: Theme.font; font.pixelSize: 18 }
                        Text { text: root.player && root.player.identity ? ("Source · " + root.player.identity) : ""; color: Qt.rgba(1, 1, 1, 0.46); font.family: Theme.font; font.pixelSize: 13 }
                    }
                }

                // Bottom-right Lyrics / Queue segmented pill (ambient mode)
                Rectangle {
                    visible: stage.ambient
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: 30; bottomMargin: 24 }
                    width: 84; height: 36; radius: 18
                    color: Qt.rgba(1, 1, 1, 0.14)
                    border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.20)
                    Row {
                        anchors.centerIn: parent; spacing: 4
                        Rectangle {
                            width: 36; height: 28; radius: 14
                            color: root.showLyricsPane ? "white" : "transparent"
                            Icon { anchors.centerIn: parent; name: "lyrics"; size: 15; color: root.showLyricsPane ? "#16161a" : "white" }
                            TapHandler { onTapped: root.showLyricsPane = true }
                        }
                        Rectangle {
                            width: 36; height: 28; radius: 14
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
                    anchors.bottomMargin: stage.ambient ? 22 : parent.height * 0.08
                    spacing: stage.ambient ? 6 : 8

                    Row {
                        visible: stage.ambient
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 10
                        Rectangle {
                            width: 34; height: 34; radius: 17
                            anchors.verticalCenter: parent.verticalCenter
                            gradient: Gradient {
                                GradientStop { position: 0; color: "#8e8e93" }
                                GradientStop { position: 1; color: "#5a5a5f" }
                            }
                            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.3)
                            Text { anchors.centerIn: parent; text: (Quickshell.env("USER") || "?").charAt(0).toUpperCase(); color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.DemiBold }
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
                        width: 240; height: 36; radius: 18
                        color: Qt.rgba(1, 1, 1, 0.16)
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.25)
                        TextField {
                            id: pw
                            anchors.fill: parent; anchors.leftMargin: 16; anchors.rightMargin: 16
                            echoMode: TextInput.Password
                            placeholderText: ""
                            color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.6)
                            font.family: Theme.font; font.pixelSize: 14
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
                            color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 14
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
