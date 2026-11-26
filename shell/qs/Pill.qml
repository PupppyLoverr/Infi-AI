import QtQuick

// A capsule button used in the island and Control Center.
Rectangle {
    id: p
    property alias text: label.text
    property bool on: false
    signal clicked
    height: 30; width: label.implicitWidth + 26; radius: 15
    color: on ? Theme.accent : Qt.rgba(1, 1, 1, 0.12)
    Text { id: label; anchors.centerIn: parent; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
    TapHandler { onTapped: p.clicked() }
}
