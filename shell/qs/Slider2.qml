import QtQuick
// A Control Center slider: a fat white fill in a glass track.
Rectangle {
    id: s
    property string label; property string icon; property real value: 0
    signal moved(real v)
    height: 64; radius: 20; color: Theme.fill
    Text { x: 12; y: 8; text: s.label; color: Theme.ink; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
    Text { visible: s.value < 0; x: 12; y: 34; text: "Not available on this device"; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody;}
    Rectangle {
        id: track
        visible: s.value >= 0
        x: 12; y: 30; width: parent.width - 24; height: 24; radius: 12; color: Qt.rgba(1, 1, 1, 0.16); clip: true
        Rectangle { width: Math.max(24, track.width * s.value); height: parent.height; radius: 12; color: Qt.rgba(1, 1, 1, 0.92) }
        Icon { x: 6; anchors.verticalCenter: parent.verticalCenter; name: s.icon; size: 13; color: "#333" }
        MouseArea {
            anchors.fill: parent
            function set(x) { s.moved(Math.max(0, Math.min(1, x / track.width))) }
            onPressed: ev => set(ev.x)
            onPositionChanged: ev => { if (pressed) set(ev.x) }
        }
    }
}
