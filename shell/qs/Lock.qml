import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pam
import Quickshell.Services.UPower

// The lock screen, Droppy-style: ambient full-screen album art (or the
// blurred wallpaper), a large clock, the notch with a lock that springs open
// on unlock, now playing with controls, synced lyrics (lrclib), battery,
// and real PAM authentication (ext-session-lock).
Scope {
    id: root
    property bool locked: false
    property string wallpaper: Theme.home + "/.config/solyra/wallpaper.png"
    property string status: ""
    property bool unlocking: false
    readonly property MprisPlayer player: {
        for (const p of Mpris.players.values) if (p.isPlaying) return p
        return Mpris.players.values.length ? Mpris.players.values[0] : null
    }

    function lock() { status = ""; unlocking = false; locked = true }

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

    // Synced lyrics for the current track from lrclib.net (LRC timestamps).
    property var lyrics: []
    property string lyricKey: ""
    onPlayerChanged: fetchLyrics()
    Connections { target: root.player; function onTrackTitleChanged() { root.fetchLyrics() } }
    function fetchLyrics() {
        if (!player || !player.trackTitle) { lyrics = []; return }
        const key = player.trackArtist + "|" + player.trackTitle
        if (key === lyricKey) return
        lyricKey = key; lyrics = []
        lyricProc.command = ["curl", "-fsS", "--max-time", "6", "-G", "https://lrclib.net/api/get",
                             "--data-urlencode", "artist_name=" + (player.trackArtist || ""),
                             "--data-urlencode", "track_name=" + player.trackTitle]
        lyricProc.running = true
    }
    Process {
        id: lyricProc
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const j = JSON.parse(text)
                    const out = []
                    for (const l of (j.syncedLyrics || "").split("\n")) {
                        const m = l.match(/^\[(\d+):(\d+(?:\.\d+)?)\]\s*(.*)$/)
                        if (m) out.push({ t: parseInt(m[1]) * 60 + parseFloat(m[2]), line: m[3] })
                    }
                    root.lyrics = out
                } catch (e) { root.lyrics = [] }
            }
        }
    }
    function lyricIndex(pos) { let i = -1; for (let k = 0; k < lyrics.length; k++) if (lyrics[k].t <= pos) i = k; return i }

    WlSessionLock {
        locked: root.locked
        WlSessionLockSurface {
            id: surf
            color: "black"
            Item {
                anchors.fill: parent
                Image {
                    id: bg
                    anchors.fill: parent
                    fillMode: Image.PreserveAspectCrop
                    source: root.player && root.player.trackArtUrl ? root.player.trackArtUrl : "file://" + root.wallpaper
                    visible: false
                }
                MultiEffect {
                    anchors.fill: parent; source: bg
                    blurEnabled: true; blur: 1.0; blurMax: 64; brightness: -0.18; saturation: 0.25
                }
                Rectangle { anchors.fill: parent; color: Qt.rgba(0, 0, 0, 0.25) }

                // Notch with the lock glyph; springs open on unlock.
                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 3; color: "black"
                    width: root.unlocking ? 150 : 100; height: 24; radius: 12
                    Behavior on width { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen } }
                    Icon { anchors.centerIn: parent; name: "lock"; size: 16; color: root.unlocking ? "#30d158" : "white"; rotation: root.unlocking ? -18 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.ms(260); easing.type: Easing.OutBack } } }
                }

                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: parent.height * 0.12
                    spacing: 0
                    Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "dddd, d MMMM"); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 22; font.weight: Font.DemiBold }
                    Text { anchors.horizontalCenter: parent.horizontalCenter; text: Qt.formatDateTime(clk.date, "H:mm"); color: "white"; font.family: Theme.font; font.pixelSize: 112; font.weight: Font.Bold }
                    SystemClock { id: clk; precision: SystemClock.Minutes }
                }

                // Synced lyrics (current line bright, next dim).
                Column {
                    visible: root.lyrics.length > 0
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: parent.height * 0.42
                    width: parent.width * 0.7
                    spacing: 10
                    property int cur: root.player ? root.lyricIndex(root.player.position) : -1
                    Timer { interval: 500; running: root.locked && root.player && root.player.isPlaying; repeat: true; onTriggered: root.player.positionChanged() }
                    Text { width: parent.width; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap; text: parent.cur >= 0 ? root.lyrics[parent.cur].line : "♪"; color: "white"; font.family: Theme.font; font.pixelSize: 34; font.weight: Font.Bold }
                    Text { width: parent.width; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap; text: parent.cur + 1 < root.lyrics.length ? root.lyrics[parent.cur + 1].line : ""; color: Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 26; font.weight: Font.DemiBold }
                }

                // Now playing card.
                Rectangle {
                    visible: !!root.player
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: parent.height * 0.62
                    width: 420; height: 96; radius: 24
                    color: Qt.rgba(1, 1, 1, 0.14); border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.18)
                    Row {
                        anchors.fill: parent; anchors.margins: 14; spacing: 14
                        Rectangle { width: 68; height: 68; radius: 14; clip: true
                            gradient: Gradient {
                                GradientStop { position: 0; color: "#ff9f43" }
                                GradientStop { position: 0.55; color: "#ee5253" }
                                GradientStop { position: 1; color: "#5f27cd" }
                            }
                            Image { anchors.fill: parent; source: root.player ? root.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop } }
                        Column { width: 190; anchors.verticalCenter: parent.verticalCenter; spacing: 2
                            Text { width: parent.width; elide: Text.ElideRight; text: root.player ? root.player.trackTitle : ""; color: "white"; font.family: Theme.font; font.pixelSize: 16; font.weight: Font.DemiBold }
                            Text { width: parent.width; elide: Text.ElideRight; text: root.player ? root.player.trackArtist : ""; color: Qt.rgba(1, 1, 1, 0.65); font.family: Theme.font; font.pixelSize: 13 } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "prev"; size: 20; color: "white"; TapHandler { onTapped: root.player.previous() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: root.player && root.player.isPlaying ? "pause" : "play"; size: 26; color: "white"; TapHandler { onTapped: root.player.togglePlaying() } }
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: "next"; size: 20; color: "white"; TapHandler { onTapped: root.player.next() } }
                    }
                }

                // Password.
                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom; anchors.bottomMargin: parent.height * 0.08
                    spacing: 8
                    // The account: avatar (initial) and name, as macOS.
                    Rectangle {
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
                        anchors.horizontalCenter: parent.horizontalCenter
                        bottomPadding: 4
                        text: { const u = Quickshell.env("USER") || "User"; return u.charAt(0).toUpperCase() + u.slice(1) }
                        color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.DemiBold
                    }
                    Rectangle {
                        id: pwBox
                        width: 240; height: 38; radius: 19
                        color: Qt.rgba(1, 1, 1, 0.18); border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.25)
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
                            onAccepted: { pam.start(); }
                            Component.onCompleted: forceActiveFocus()
                        }
                        // Our own hint: Qt hides placeholders while focused, and
                        // this field is always focused.
                        Text {
                            anchors.centerIn: parent
                            visible: pw.text === "" && !pam.active
                            text: "Enter Password"
                            color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 14
                        }
                        Connections {
                            target: pam
                            function onResponseRequiredChanged() { if (pam.responseRequired) { pam.respond(pw.text); pw.text = "" } }
                        }
                    }
                    Text { anchors.horizontalCenter: parent.horizontalCenter; text: root.status; color: "#ff8f8f"; font.family: Theme.font; font.pixelSize: 12 }
                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: 6
                        visible: UPower.displayDevice.isLaptopBattery
                        Icon { name: UPower.displayDevice.state === UPowerDeviceState.Charging ? "charging" : "battery"; size: 16; color: "white" }
                        Text { text: Math.round(UPower.displayDevice.percentage * (UPower.displayDevice.percentage > 1 ? 1 : 100)) + "%"; color: "white"; font.family: Theme.font; font.pixelSize: 12 }
                    }
                }
            }
        }
    }
}
