import QtQuick
import Quickshell

// macOS 27 Golden Gate menu bar title: subtle Liquid Glass pill hover/open
// highlight + Liquid Glass dropdown menu with right-aligned shortcut hints.
Item {
    id: t
    property string label
    property bool bold: false
    property var items: []
    property var shell
    width: txt.implicitWidth + 16
    height: 24

    Rectangle {
        anchors.fill: parent
        radius: 7
        color: menu.visible
            ? Qt.rgba(1, 1, 1, Theme.dark ? 0.22 : 0.36)
            : titleHov.hovered
                ? Qt.rgba(1, 1, 1, Theme.dark ? 0.14 : 0.24)
                : "transparent"
        Behavior on color { ColorAnimation { duration: Theme.ms(110) } }
    }

    Text {
        id: txt
        anchors.centerIn: parent
        text: t.label
        color: Theme.ink
        font.family: Theme.font
        font.pixelSize: 13
        font.weight: t.bold ? Font.Bold : Font.Medium
    }

    HoverHandler { id: titleHov }
    TapHandler { onTapped: menu.visible = !menu.visible }

    PopupWindow {
        id: menu
        anchor.item: t
        anchor.rect.y: 28
        implicitWidth: 244
        implicitHeight: col.implicitHeight + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 14
            Column {
                id: col
                x: 6; y: 6; width: parent.width - 12; spacing: 1
                Repeater {
                    model: t.items
                    Item {
                        required property var modelData
                        width: col.width; height: modelData[0] === "-" ? 9 : 26
                        Rectangle {
                            visible: modelData[0] === "-"
                            anchors.centerIn: parent
                            width: parent.width - 14; height: 1
                            color: Qt.rgba(1, 1, 1, Theme.dark ? 0.13 : 0.14)
                        }
                        Rectangle {
                            visible: modelData[0] !== "-"
                            anchors.fill: parent; radius: 7
                            color: hh.hovered ? Theme.accent : "transparent"
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                x: 10
                                text: modelData[0]
                                color: hh.hovered ? "white" : Theme.ink
                                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Normal
                            }
                            Text {
                                visible: modelData.length > 2 && modelData[2] !== ""
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.right: parent.right
                                anchors.rightMargin: 10
                                text: modelData.length > 2 ? modelData[2] : ""
                                color: hh.hovered ? Qt.rgba(1, 1, 1, 0.85) : Theme.ink3
                                font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                            }
                            HoverHandler { id: hh }
                            TapHandler { onTapped: { menu.visible = false; if (t.shell) t.shell.menuAction(modelData[1]) } }
                        }
                    }
                }
            }
        }
    }
}
