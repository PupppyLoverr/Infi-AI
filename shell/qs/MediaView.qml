import QtQuick
import Quickshell
import Quickshell.Services.Mpris
import Quickshell.Widgets

// Droppy-style Notch Media Player: album art, bold title + clickable [L] synced
// lyrics badge, warm equaliser, seekable scrubber, 5-button transport row
// (Shuffle, Prev, Play/Pause, Next, Repeat) + AirPlay output indicator, and an
// expandable live synced lyrics drawer with expand-to-fullscreen button.
Item {
    id: m
    property MprisPlayer player
    property var lyrics
    property bool lyricsOpen: false
    property bool shuffleOn: false
    property string loopMode: "None" // None | Playlist | Track
    signal expandLock()

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

    readonly property int lyricCur: m.player && m.lyrics ? m.lyrics.indexAt(m.player.position) : -1
    readonly property var lyricLines: m.lyrics ? m.lyrics.lines : []

    Timer {
        interval: 450
        running: m.visible && m.lyricsOpen && m.player !== null && m.player.isPlaying
        repeat: true
        onTriggered: if (m.player) m.player.positionChanged()
    }

    Text {
        visible: !m.player
        anchors.centerIn: parent
        text: "Nothing is playing"
        color: Qt.rgba(1, 1, 1, 0.5)
        font.family: Theme.font
        font.pixelSize: 14
    }

    Column {
        visible: !!m.player
        anchors.fill: parent
        spacing: 12

        // Header: album art, title + [L] badge, artist, equaliser.
        Row {
            spacing: 14
            width: parent.width
            ClippingRectangle {
                width: 60; height: 60; radius: 14
                color: "#ee5253"
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        GradientStop { position: 0; color: "#ff9f43" }
                        GradientStop { position: 0.55; color: "#ee5253" }
                        GradientStop { position: 1; color: "#5f27cd" }
                    }
                }
                Image {
                    anchors.fill: parent
                    source: m.player ? m.player.trackArtUrl : ""
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                }
            }
            Column {
                width: parent.width - 60 - 14 - 40
                anchors.verticalCenter: parent.verticalCenter
                spacing: 3
                Row {
                    width: parent.width
                    spacing: 7
                    Text {
                        width: Math.min(implicitWidth, parent.width - 28)
                        elide: Text.ElideRight
                        text: m.player ? (m.player.trackTitle || "Unknown title") : ""
                        color: "white"
                        font.family: Theme.font
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                    }
                    // Clickable [L] Live Synced Lyrics toggle badge (Droppy)
                    Rectangle {
                        width: 20; height: 18; radius: 5
                        anchors.verticalCenter: parent.verticalCenter
                        color: m.lyricsOpen ? "white" : Qt.rgba(1, 1, 1, 0.16)
                        border.width: m.lyricsOpen ? 0 : 1
                        border.color: Qt.rgba(1, 1, 1, 0.24)
                        Behavior on color { ColorAnimation { duration: Theme.ms(140) } }
                        Text {
                            anchors.centerIn: parent
                            text: "L"
                            color: m.lyricsOpen ? "#111114" : Qt.rgba(1, 1, 1, 0.88)
                            font.family: Theme.font
                            font.pixelSize: 10
                            font.weight: Font.Bold
                        }
                        TapHandler { onTapped: m.lyricsOpen = !m.lyricsOpen }
                    }
                }
                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    text: m.player ? (m.player.trackArtist || m.player.identity) : ""
                    color: Qt.rgba(1, 1, 1, 0.62)
                    font.family: Theme.font
                    font.pixelSize: 13
                }
            }
            Equaliser {
                anchors.verticalCenter: parent.verticalCenter
                active: m.player && m.player.isPlaying
            }
        }

        // Seekable scrubber bar.
        Row {
            width: parent.width
            spacing: 10
            Text {
                width: 36
                text: m.fmt(m.player ? m.player.position : -1)
                color: Qt.rgba(1, 1, 1, 0.6)
                font.family: Theme.font
                font.pixelSize: 11
                font.features: { "tnum": 1 }
            }
            Rectangle {
                id: track
                width: parent.width - 92; height: 6; radius: 3
                anchors.verticalCenter: parent.verticalCenter
                color: Qt.rgba(1, 1, 1, 0.2)
                Rectangle {
                    height: parent.height; radius: 3; color: "white"
                    width: m.player && m.player.length > 0 ? parent.width * Math.min(1, Math.max(0, m.player.position / m.player.length)) : 0
                }
                TapHandler {
                    onTapped: ev => {
                        if (m.player && m.player.canSeek && m.player.length > 0)
                            m.player.position = ev.position.x / track.width * m.player.length
                    }
                }
            }
            Text {
                width: 40
                horizontalAlignment: Text.AlignRight
                text: m.player && m.player.length > 0 ? "-" + m.fmt(Math.max(0, m.player.length - m.player.position)) : "--:--"
                color: Qt.rgba(1, 1, 1, 0.6)
                font.family: Theme.font
                font.pixelSize: 11
                font.features: { "tnum": 1 }
            }
        }

        // 5-button Droppy transport row: Shuffle, Prev, Play/Pause, Next, Repeat (+ Lyrics/AirPlay)
        Item {
            width: parent.width
            height: 32
            Row {
                anchors.centerIn: parent
                spacing: 30
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "shuffle"; size: 17
                    color: m.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.55)
                    TapHandler { onTapped: m.toggleShuffle() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "prev"; size: 22; color: "white"
                    TapHandler { onTapped: if (m.player && m.player.canGoPrevious) m.player.previous() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: m.player && m.player.isPlaying ? "pause" : "play"
                    size: 28; color: "white"
                    TapHandler { onTapped: if (m.player) m.player.togglePlaying() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "next"; size: 22; color: "white"
                    TapHandler { onTapped: if (m.player && m.player.canGoNext) m.player.next() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "repeat"; size: 17
                    color: m.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.55)
                    TapHandler { onTapped: m.cycleLoop() }
                }
            }
            Icon {
                anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                name: "lyrics"; size: 17
                color: m.lyricsOpen ? Theme.accent : Qt.rgba(1, 1, 1, 0.52)
                TapHandler { onTapped: m.lyricsOpen = !m.lyricsOpen }
            }
            Icon {
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                name: "airplay"; size: 17
                color: Qt.rgba(1, 1, 1, 0.55)
                TapHandler { onTapped: m.expandLock() }
            }
        }

        // Droppy Live Synced Lyrics Drawer (expanded when [L] is toggled on)
        Rectangle {
            visible: m.lyricsOpen
            width: parent.width; height: 1
            color: Qt.rgba(1, 1, 1, 0.12)
        }
        Item {
            visible: m.lyricsOpen
            width: parent.width
            height: 118
            clip: true

            Text {
                visible: m.lyricLines.length === 0
                anchors.centerIn: parent
                text: m.lyrics && m.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                color: Qt.rgba(1, 1, 1, 0.45)
                font.family: Theme.font
                font.pixelSize: 13
            }

            Column {
                visible: m.lyricLines.length > 0
                anchors.left: parent.left
                anchors.right: expandBtn.left
                anchors.rightMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                Text {
                    width: parent.width; elide: Text.ElideRight
                    visible: m.lyricCur > 0
                    text: m.lyricCur > 0 && m.lyricCur - 1 < m.lyricLines.length ? m.lyricLines[m.lyricCur - 1].line : ""
                    color: Qt.rgba(1, 1, 1, 0.36)
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
                }
                Text {
                    width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight
                    text: m.lyricCur >= 0 && m.lyricCur < m.lyricLines.length ? m.lyricLines[m.lyricCur].line : m.lyricLines[0].line
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 16; font.weight: Font.Bold
                }
                Text {
                    width: parent.width; elide: Text.ElideRight
                    visible: m.lyricCur + 1 < m.lyricLines.length
                    text: m.lyricCur + 1 >= 0 && m.lyricCur + 1 < m.lyricLines.length ? m.lyricLines[m.lyricCur + 1].line : ""
                    color: Qt.rgba(1, 1, 1, 0.44)
                    font.family: Theme.font; font.pixelSize: 13.5; font.weight: Font.Medium
                }
                Text {
                    width: parent.width; elide: Text.ElideRight
                    visible: m.lyricCur + 2 < m.lyricLines.length
                    text: m.lyricCur + 2 >= 0 && m.lyricCur + 2 < m.lyricLines.length ? m.lyricLines[m.lyricCur + 2].line : ""
                    color: Qt.rgba(1, 1, 1, 0.28)
                    font.family: Theme.font; font.pixelSize: 12.5
                }
            }

            // Expand-to-fullscreen ambient lyrics button (Droppy)
            Rectangle {
                id: expandBtn
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                width: 28; height: 28; radius: 14
                color: Qt.rgba(1, 1, 1, 0.14)
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.20)
                Icon { anchors.centerIn: parent; name: "expand"; size: 14; color: "white" }
                TapHandler { onTapped: m.expandLock() }
            }
        }
    }
}
