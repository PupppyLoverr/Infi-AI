import QtQuick
import Quickshell.Services.Mpris
import Quickshell.Widgets

// Now playing: art, title, artist, a seekable scrubber and transport.
Item {
    id: m
    property MprisPlayer player
    function fmt(s) { if (!(s >= 0)) return "--:--"; s = Math.floor(s); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0") }
    Text {
        visible: !m.player
        anchors.centerIn: parent
        text: "Nothing is playing"; color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 14
    }
    Column {
        visible: !!m.player
        anchors.fill: parent
        spacing: 14
        Row {
            spacing: 14
            width: parent.width
            ClippingRectangle {
                width: 64; height: 64; radius: 14
                color: "#ee5253"
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        GradientStop { position: 0; color: "#ff9f43" }
                        GradientStop { position: 0.55; color: "#ee5253" }
                        GradientStop { position: 1; color: "#5f27cd" }
                    }
                }
                Image { anchors.fill: parent; source: m.player ? m.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop; asynchronous: true }
            }
            Column {
                width: parent.width - 64 - 14 - 40
                anchors.verticalCenter: parent.verticalCenter
                spacing: 3
                Text { width: parent.width; elide: Text.ElideRight; text: m.player ? (m.player.trackTitle || "Unknown title") : ""; color: "white"; font.family: Theme.font; font.pixelSize: 17; font.weight: Font.DemiBold }
                Text { width: parent.width; elide: Text.ElideRight; text: m.player ? (m.player.trackArtist || m.player.identity) : ""; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 14 }
            }
            Equaliser { anchors.verticalCenter: parent.verticalCenter; active: m.player && m.player.isPlaying }
        }
        Row {
            width: parent.width
            spacing: 10
            Text { width: 36; text: m.fmt(m.player ? m.player.position : -1); color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 11 }
            Rectangle {
                id: track
                width: parent.width - 92; height: 6; radius: 3
                anchors.verticalCenter: parent.verticalCenter
                color: Qt.rgba(1, 1, 1, 0.2)
                Rectangle {
                    height: parent.height; radius: 3; color: "white"
                    width: m.player && m.player.length > 0 ? parent.width * Math.min(1, m.player.position / m.player.length) : 0
                }
                TapHandler {
                    onTapped: ev => { if (m.player && m.player.canSeek && m.player.length > 0) m.player.position = ev.position.x / track.width * m.player.length }
                }
            }
            Text { width: 40; horizontalAlignment: Text.AlignRight; text: m.player && m.player.length > 0 ? "-" + m.fmt(m.player.length - m.player.position) : "--:--"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 11 }
        }
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 40
            Icon { name: "prev"; size: 24; color: "white"; TapHandler { onTapped: if (m.player && m.player.canGoPrevious) m.player.previous() } }
            Icon { name: m.player && m.player.isPlaying ? "pause" : "play"; size: 30; color: "white"; TapHandler { onTapped: if (m.player) m.player.togglePlaying() } }
            Icon { name: "next"; size: 24; color: "white"; TapHandler { onTapped: if (m.player && m.player.canGoNext) m.player.next() } }
        }
    }
}
