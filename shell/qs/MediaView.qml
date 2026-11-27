import QtQuick
import Quickshell
import Quickshell.Services.Mpris
import Quickshell.Widgets

// Exact getdroppy.app Notch Media Player & Live Synced Lyrics Drawer on pure
// OLED black: compact 48x48 art, bold title + clickable white [L] badge, coral
// equalizer, seekable scrubber, 5-button Droppy transport row (Queue/Shuffle,
// Prev, Play/Pause, Next, AirPlay/Repeat), and sleek vertical synced lyrics
// drawer with circular expand-to-ambient-lock button.
Item {
    id: m
    property MprisPlayer player
    property var lyrics
    property bool lyricsOpen: false
    property bool shuffleOn: false
    property string loopMode: "None"
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
    onLyricsOpenChanged: if (lyricsOpen && m.lyrics) m.lyrics.refresh()

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
        color: Qt.rgba(1, 1, 1, 0.45)
        font.family: Theme.font
        font.pixelSize: 14
    }

    Column {
        visible: !!m.player
        anchors.fill: parent
        spacing: 11

        // Header: 48x48 album art, title + [L] badge, artist, warm coral equaliser.
        Row {
            spacing: 12
            width: parent.width
            ClippingRectangle {
                width: 48; height: 48; radius: 11
                color: "#1a1a22"
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
                width: parent.width - 48 - 12 - 36
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                Row {
                    width: parent.width
                    spacing: 6
                    Text {
                        width: Math.min(implicitWidth, parent.width - 26)
                        elide: Text.ElideRight
                        text: m.player ? (m.player.trackTitle || "Unknown title") : ""
                        color: "white"
                        font.family: Theme.font
                        font.pixelSize: 15
                        font.weight: Font.Bold
                    }
                    // Droppy signature white [L] badge toggling synced lyrics
                    Rectangle {
                        width: 19; height: 17; radius: 4
                        anchors.verticalCenter: parent.verticalCenter
                        color: m.lyricsOpen ? "white" : Qt.rgba(1, 1, 1, 0.85)
                        opacity: m.lyricsOpen ? 1.0 : 0.72
                        Text {
                            anchors.centerIn: parent
                            text: "L"
                            color: "#000000"
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
                    color: Qt.rgba(1, 1, 1, 0.56)
                    font.family: Theme.font
                    font.pixelSize: 12
                }
            }
            Equaliser {
                anchors.verticalCenter: parent.verticalCenter
                active: m.player && m.player.isPlaying
                tint: "#ff7a45"
            }
        }

        // Scrubber row: elapsed, smooth white progress bar + thumb, remaining.
        Row {
            width: parent.width
            spacing: 8
            Text {
                width: 30
                text: m.fmt(m.player ? m.player.position : -1)
                color: Qt.rgba(1, 1, 1, 0.50)
                font.family: Theme.font
                font.pixelSize: 10
                font.features: { "tnum": 1 }
            }
            Item {
                id: track
                width: parent.width - 78; height: 14
                anchors.verticalCenter: parent.verticalCenter
                readonly property real frac: m.player && m.player.length > 0 ? Math.min(1, Math.max(0, m.player.position / m.player.length)) : 0
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width; height: 5; radius: 2.5
                    color: Qt.rgba(1, 1, 1, 0.22)
                    Rectangle {
                        height: parent.height; radius: 2.5; color: Qt.rgba(1, 1, 1, 0.88)
                        width: parent.width * track.frac
                    }
                }
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    x: Math.max(0, Math.min(parent.width - width, parent.width * track.frac - width / 2))
                    width: 10; height: 10; radius: 5
                    color: "white"
                }
                TapHandler {
                    onTapped: ev => {
                        if (m.player && m.player.canSeek && m.player.length > 0)
                            m.player.position = ev.position.x / track.width * m.player.length
                    }
                }
            }
            Text {
                width: 32
                horizontalAlignment: Text.AlignRight
                text: m.player && m.player.length > 0 ? "-" + m.fmt(Math.max(0, m.player.length - m.player.position)) : "--:--"
                color: Qt.rgba(1, 1, 1, 0.50)
                font.family: Theme.font
                font.pixelSize: 10
                font.features: { "tnum": 1 }
            }
        }

        // Droppy 5-button transport row: Queue/Shuffle, Prev, Play/Pause, Next, Output/Repeat
        Item {
            width: parent.width
            height: 36
            Icon {
                anchors.left: parent.left; anchors.leftMargin: 6; anchors.verticalCenter: parent.verticalCenter
                name: m.lyricsOpen ? "queue" : "shuffle"; size: 17
                color: m.shuffleOn ? Theme.accent : Qt.rgba(1, 1, 1, 0.56)
                TapHandler { onTapped: m.toggleShuffle() }
            }
            Row {
                anchors.centerIn: parent
                spacing: 28
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "prev"; size: 22; color: "white"
                    TapHandler { onTapped: if (m.player && m.player.canGoPrevious) m.player.previous() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: m.player && m.player.isPlaying ? "pause" : "play"
                    size: 30; color: "white"
                    TapHandler { onTapped: if (m.player) m.player.togglePlaying() }
                }
                Icon {
                    anchors.verticalCenter: parent.verticalCenter
                    name: "next"; size: 22; color: "white"
                    TapHandler { onTapped: if (m.player && m.player.canGoNext) m.player.next() }
                }
            }
            Icon {
                anchors.right: parent.right; anchors.rightMargin: 6; anchors.verticalCenter: parent.verticalCenter
                name: m.loopMode !== "None" ? "repeat" : "airplay"; size: 18
                color: m.loopMode !== "None" ? Theme.accent : Qt.rgba(1, 1, 1, 0.62)
                TapHandler { onTapped: m.cycleLoop() }
            }
        }

        // Droppy Live Synced Lyrics Drawer on pure pitch black (exact parity!)
        Item {
            visible: m.lyricsOpen
            width: parent.width
            height: 140
            clip: true

            Text {
                visible: m.lyricLines.length === 0
                anchors.centerIn: parent
                text: m.lyrics && m.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                color: Qt.rgba(1, 1, 1, 0.42)
                font.family: Theme.font
                font.pixelSize: 13
            }

            Column {
                visible: m.lyricLines.length > 0
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.rightMargin: 32
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                Text {
                    width: parent.width; elide: Text.ElideRight
                    visible: m.lyricCur > 0
                    text: m.lyricCur > 0 && m.lyricCur - 1 < m.lyricLines.length ? m.lyricLines[m.lyricCur - 1].line : ""
                    color: Qt.rgba(1, 1, 1, 0.34)
                    font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold
                }
                Text {
                    width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight
                    text: m.lyricLines.length > 0 ? m.lyricLines[m.lyricCur >= 0 && m.lyricCur < m.lyricLines.length ? m.lyricCur : 0].line : ""
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 16; font.weight: Font.Bold
                }
                Text {
                    width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight
                    visible: m.lyricCur + 1 < m.lyricLines.length
                    text: m.lyricCur + 1 >= 0 && m.lyricCur + 1 < m.lyricLines.length ? m.lyricLines[m.lyricCur + 1].line : ""
                    color: Qt.rgba(1, 1, 1, 0.38)
                    font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium
                }
                Text {
                    width: parent.width; elide: Text.ElideRight
                    visible: m.lyricCur + 2 < m.lyricLines.length
                    text: m.lyricCur + 2 >= 0 && m.lyricCur + 2 < m.lyricLines.length ? m.lyricLines[m.lyricCur + 2].line : ""
                    color: Qt.rgba(1, 1, 1, 0.24)
                    font.family: Theme.font; font.pixelSize: 13
                }
            }

            // Bottom-right circular expand-to-ambient-lock button (exact Droppy match)
            Rectangle {
                id: expandBtn
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                width: 26; height: 26; radius: 13
                color: Qt.rgba(1, 1, 1, 0.14)
                Icon { anchors.centerIn: parent; name: "expand"; size: 13; color: Qt.rgba(1, 1, 1, 0.85) }
                TapHandler { onTapped: m.expandLock() }
            }
        }
    }
}
