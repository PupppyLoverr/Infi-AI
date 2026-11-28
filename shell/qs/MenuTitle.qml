import QtQuick
import Quickshell

// A menu bar title (macOS 26/27): white text on the transparent bar, a soft
// glass capsule behind it while its menu is open, and a clear-glass dropdown
// whose rows highlight in the accent. Actions are real: window management
// through Hyprland, keystrokes through wtype, shell actions through the
// shell itself.
Item {
    id: t
    property string label
    property string icon: ""
    property bool bold: false
    property var items: []
    property var shell
    readonly property bool open: menu.visible
    width: (icon ? 16 : txt.implicitWidth) + 16; height: 28

    // The open-item capsule.
    Rectangle {
        anchors.fill: parent
        radius: 6
        color: Qt.rgba(1, 1, 1, t.open ? 0.18 : (hov.hovered ? 0.12 : 0))
        Behavior on color { ColorAnimation { duration: Theme.ms(120) } }
    }
    Text {
        id: txt
        visible: !t.icon
        anchors.centerIn: parent
        text: t.label; color: "white"
        font.family: Theme.font; font.pixelSize: 13; font.weight: t.bold ? Theme.wBold : Theme.wBody
        style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.18)
    }
    Icon { visible: !!t.icon; anchors.centerIn: parent; name: t.icon || "info"; size: 16 }
    HoverHandler { id: hov }
    TapHandler { onTapped: menu.visible = !menu.visible }

    PopupWindow {
        id: menu
        anchor.item: t
        anchor.rect.y: t.height + 6
        implicitWidth: 250
        implicitHeight: col.height + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 14; kind: "clear"
            Column {
                id: col
                x: 6; y: 6; width: parent.width - 12
                Repeater {
                    model: t.items
                    Item {
                        required property var modelData
                        width: col.width; height: modelData[0] === "-" ? 11 : 26
                        Rectangle { visible: modelData[0] === "-"; anchors.centerIn: parent; width: parent.width - 20; height: 1; color: Qt.rgba(1, 1, 1, 0.14) }
                        Rectangle {
                            visible: modelData[0] !== "-"
                            anchors.fill: parent; radius: 8
                            color: hh.hovered ? Theme.accent : "transparent"
                            Text { anchors.verticalCenter: parent.verticalCenter; x: 12; text: modelData[0]; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wBody;}
                            Text { visible: !!modelData[2]; anchors.verticalCenter: parent.verticalCenter; anchors.right: parent.right; anchors.rightMargin: 12; text: modelData[2] || ""; color: hh.hovered ? "white" : Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wBody;}
                            HoverHandler { id: hh }
                            TapHandler { onTapped: { menu.visible = false; t.shell.menuAction(modelData[1]) } }
                        }
                    }
                }
            }
        }
    }
}
