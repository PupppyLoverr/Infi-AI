import QtQuick
import Quickshell

// A menu bar status item (battery, Wi-Fi, Focus…): its symbol on the bar, a
// glass capsule behind it while open, and a clear-glass panel below holding
// whatever the item shows (default property: the panel's content).
Item {
    id: s
    property string icon
    property real iconSize: 16
    property int panelWidth: 340
    property int panelHeight: 200
    property bool open: false
    property Component symbol: null        // optional custom bar symbol (battery)
    default property alias content: holder.data
    signal opened()
    width: 30; height: 24

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: Qt.rgba(1, 1, 1, s.open ? 0.20 : (hov.hovered ? 0.08 : 0))
        Behavior on color { ColorAnimation { duration: Theme.ms(120) } }
    }
    Icon { visible: !s.symbol; anchors.centerIn: parent; name: s.icon || "info"; size: s.iconSize }
    Loader { anchors.centerIn: parent; active: !!s.symbol; sourceComponent: s.symbol }
    HoverHandler { id: hov }
    TapHandler { onTapped: { s.open = !s.open; if (s.open) s.opened() } }

    PopupWindow {
        anchor.item: s
        anchor.rect.x: s.width - s.panelWidth + 8
        anchor.rect.y: s.height + 8
        visible: s.open
        implicitWidth: s.panelWidth
        implicitHeight: s.panelHeight
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 22; kind: "clear"
            Item { id: holder; anchors.fill: parent; anchors.margins: 16 }
        }
    }
}
