import QtQuick
import Quickshell
import Quickshell.Services.Mpris
import Quickshell.Widgets

// Exact getdroppy.app Notch Media Player + Live Synced Lyrics Drawer + Playing Next Queue:
// - 44x44 rounded art, bold title + solid white [L] lyrics badge, coral 6-bar EQ
// - Seekable rounded scrubber (`0:04` — track + thumb — `-3:20`)
// - 5-button Droppy transport row: Queue (highlighted rounded square when open),
//   Prev, Play/Pause, Next, Output Device (cycles PipeWire audio sinks)
// - Mutually exclusive Live Synced Lyrics Drawer ([L]) & Playing Next Queue Drawer (≡)
Item {
    id: m
    property MprisPlayer player
    property var lyrics
    property var shell
    property bool lyricsOpen: false
    property bool queueOpen: false
    signal expandLock()
    signal toast(string title, string detail)

    function fmt(s) {
        if (!(s >= 0)) return "--:--"
        s = Math.floor(s)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    function toggleLyrics() {
        lyricsOpen = !lyricsOpen
        if (lyricsOpen) queueOpen = false
    }

    function toggleQueue() {
        queueOpen = !queueOpen
        if (queueOpen) lyricsOpen = false
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
        m.toast("Audio Output", "Switched output device")
    }

    readonly property int lyricCur: m.player && m.lyrics ? m.lyrics.indexAt(m.player.position) : -1
    readonly property var lyricLines: m.lyrics ? m.lyrics.lines : []
    readonly property var queueItems: m.shell ? m.shell.mediaQueue : []
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
        font.weight: Theme.wBody
    }

    Column {
        visible: !!m.player
        anchors.fill: parent
        spacing: 11

        // Header: 44x44 album art, bold title + crisp white [L] badge, artist, coral 6-bar EQ
        Row {
            spacing: 12
            width: parent.width
            ClippingRectangle {
                width: 44; height: 44; radius: 10
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
                width: parent.width - 44 - 12 - 36
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                Row {
                    width: parent.width
                    spacing: 6
                    Text {
                        width: Math.min(implicitWidth, parent.width - 24)
                        elide: Text.ElideRight
                        text: m.player ? (m.player.trackTitle || "Unknown title") : ""
                        color: "white"
                        font.family: Theme.font
                        font.pixelSize: 15
                        font.weight: Font.Bold
                    }
                    // Droppy signature solid white [L] badge toggling synced lyrics
                    Rectangle {
                        width: 18; height: 17; radius: 4
                        anchors.verticalCenter: parent.verticalCenter
                        color: m.lyricsOpen ? "#ffffff" : Qt.rgba(1, 1, 1, 0.88)
                        Text {
                            anchors.centerIn: parent
                            text: "L"
                            color: "#000000"
                            font.family: Theme.font
                            font.pixelSize: 10
                            font.weight: Font.Bold
                        }
                        TapHandler { onTapped: m.toggleLyrics() }
                    }
                }
                Text {
                    width: parent.width
                    elide: Text.ElideRight
                    text: m.player ? (m.player.trackArtist || m.player.identity) : ""
                    color: Qt.rgba(1, 1, 1, 0.58)
                    font.family: Theme.font
                    font.pixelSize: 12
                    font.weight: Theme.wBody
                }
            }
            Equaliser {
                anchors.verticalCenter: parent.verticalCenter
                active: m.player && m.player.isPlaying
                tint: "#d95b63"
            }
        }

        // Scrubber row: elapsed, smooth rounded bar + thumb, remaining
        Row {
            width: parent.width
            spacing: 8
            Text {
                width: 30
                text: m.fmt(m.player ? m.player.position : -1)
                color: Qt.rgba(1, 1, 1, 0.52)
                font.family: Theme.font
                font.pixelSize: 10
                font.features: { "tnum": 1 }
                font.weight: Theme.wBody
            }
            Item {
                id: track
                width: parent.width - 78; height: 14
                anchors.verticalCenter: parent.verticalCenter
                readonly property real frac: m.player && m.player.length > 0 ? Math.min(1, Math.max(0, m.player.position / m.player.length)) : 0
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: parent.width; height: 6; radius: 3
                    color: Qt.rgba(1, 1, 1, 0.20)
                    Rectangle {
                        height: parent.height; radius: 3; color: Qt.rgba(1, 1, 1, 0.82)
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
                color: Qt.rgba(1, 1, 1, 0.52)
                font.family: Theme.font
                font.pixelSize: 10
                font.features: { "tnum": 1 }
                font.weight: Theme.wBody
            }
        }

        // Droppy 5-button transport row: Queue (rounded highlight box when active), Prev, Play/Pause, Next, Output Device
        Item {
            width: parent.width
            height: 36
            Rectangle {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                width: 38; height: 32; radius: 10
                color: m.queueOpen ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
                Icon {
                    anchors.centerIn: parent
                    name: "queue"; size: 18
                    color: m.queueOpen ? "white" : Qt.rgba(1, 1, 1, 0.66)
                }
                TapHandler { onTapped: m.toggleQueue() }
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
            Rectangle {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                width: 38; height: 32; radius: 10
                color: "transparent"
                Icon {
                    anchors.centerIn: parent
                    name: "airpods"; size: 19
                    color: Qt.rgba(1, 1, 1, 0.72)
                }
                TapHandler { onTapped: m.cycleOutput() }
            }
        }

        // Droppy Live Synced Lyrics Drawer ([L])
        Item {
            visible: m.lyricsOpen
            width: parent.width
            height: 146
            clip: true

            Text {
                visible: m.lyricLines.length === 0
                anchors.centerIn: parent
                text: m.lyrics && m.lyrics.loading ? "Loading synced lyrics…" : "No synced lyrics for this track"
                color: Qt.rgba(1, 1, 1, 0.42)
                font.family: Theme.font
                font.pixelSize: 13
                font.weight: Theme.wBody
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
                    font.weight: Theme.wBody
                }
            }

            Rectangle {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                width: 26; height: 26; radius: 13
                color: Qt.rgba(1, 1, 1, 0.14)
                Icon { anchors.centerIn: parent; name: "expand"; size: 13; color: Qt.rgba(1, 1, 1, 0.85) }
                TapHandler { onTapped: m.expandLock() }
            }
        }

        // Droppy Playing Next Queue Drawer (exact match to 17_droppy_notch_playing_next_queue.png!)
        Column {
            visible: m.queueOpen
            width: parent.width
            spacing: 10

            Rectangle {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 72; height: 1; color: Qt.rgba(1, 1, 1, 0.12)
            }

            Text {
                text: "Playing Next"
                color: Qt.rgba(1, 1, 1, 0.56)
                font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
            }

            Column {
                width: parent.width
                spacing: 8
                Repeater {
                    model: m.queueItems.slice(0, 4)
                    Item {
                        required property var modelData
                        required property int index
                        width: parent.width
                        height: 40
                        Row {
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.rightMargin: 28
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 11
                            Rectangle {
                                width: 36; height: 36; radius: 8
                                gradient: Gradient {
                                    GradientStop { position: 0; color: modelData.c1 || "#2563eb" }
                                    GradientStop { position: 1; color: modelData.c2 || "#06b6d4" }
                                }
                                Icon { anchors.centerIn: parent; name: "music"; size: 15; color: Qt.rgba(1, 1, 1, 0.88) }
                            }
                            Column {
                                anchors.verticalCenter: parent.verticalCenter
                                width: parent.width - 48
                                spacing: 1
                                Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold }
                                Text { width: parent.width; elide: Text.ElideRight; text: modelData.artist; color: Qt.rgba(1, 1, 1, 0.52); font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody;}
                            }
                        }
                        Icon {
                            visible: index > 0
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            name: "list"; size: 15; color: Qt.rgba(1, 1, 1, 0.42)
                        }
                        TapHandler { onTapped: if (m.shell) m.shell.playQueueItem(index) }
                    }
                }
            }
        }
    }
}
