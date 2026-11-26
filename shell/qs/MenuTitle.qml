import QtQuick
import Quickshell

// A menu bar title with a glass dropdown. Actions are real: window
// management through Hyprland, keystrokes through wtype, shell actions
// through the shell's IPC.
Item {
    id: t
    property string label
    property bool bold: false
    property var items: []
    property var shell
    width: txt.implicitWidth; height: 30
    Text { id: txt; anchors.verticalCenter: parent.verticalCenter; text: t.label; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: t.bold ? Font.Bold : Font.Normal }
    TapHandler { onTapped: menu.visible = !menu.visible }
    PopupWindow {
        id: menu
        anchor.item: t
        anchor.rect.y: 30
        implicitWidth: 230
        implicitHeight: col.height + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 12
            Column {
                id: col
                x: 6; y: 6; width: parent.width - 12
                Repeater {
                    model: t.items
                    Item {
                        required property var modelData
                        width: col.width; height: modelData[0] === "-" ? 9 : 26
                        Rectangle { visible: modelData[0] === "-"; anchors.centerIn: parent; width: parent.width - 12; height: 1; color: Theme.fill }
                        Rectangle {
                            visible: modelData[0] !== "-"
                            anchors.fill: parent; radius: 6
                            color: hh.hovered ? Theme.accent : "transparent"
                            Text { anchors.verticalCenter: parent.verticalCenter; x: 10; text: modelData[0]; color: hh.hovered ? "white" : Theme.ink; font.family: Theme.font; font.pixelSize: 13 }
                            HoverHandler { id: hh }
                            TapHandler { onTapped: { menu.visible = false; t.shell.menuAction(modelData[1]) } }
                        }
                    }
                }
            }
        }
    }
}
