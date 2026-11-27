import QtQuick
import Quickshell
import Quickshell.Wayland

// Droppy Floating Drop Basket: detachable Liquid Glass drop-zone floating on the
// right screen edge so users can park & drag files anywhere across workspaces.
PanelWindow {
    id: win
    property var shell
    property var model
    visible: shell !== null && shell.basketOpen
    anchors { right: true; top: true }
    margins { right: 18; top: 140 }
    implicitWidth: 212
    implicitHeight: 248
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-basket"

    property bool dropping: false

    Glass {
        anchors.fill: parent
        r: 22
        dark: true

        DropArea {
            anchors.fill: parent
            onEntered: win.dropping = true
            onExited: win.dropping = false
            onDropped: drop => {
                win.dropping = false
                for (const u of drop.urls) if (win.model) win.model.add(u.toString())
            }
        }

        Column {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 10

            Item {
                width: parent.width; height: 24
                Row {
                    anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; spacing: 6
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "basket"; size: 14; color: "#0a84ff" }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: "Basket (" + (win.model ? win.model.count : 0) + ")"; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold }
                }
                Rectangle {
                    anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                    width: 22; height: 22; radius: 11; color: Qt.rgba(1, 1, 1, 0.14)
                    Icon { anchors.centerIn: parent; name: "x"; size: 11; color: "white" }
                    TapHandler { onTapped: if (win.shell) win.shell.basketOpen = false }
                }
            }

            Rectangle {
                visible: !win.model || win.model.count === 0 || win.dropping
                width: parent.width; height: parent.height - 34; radius: 14
                color: Qt.rgba(1, 1, 1, 0.05)
                border.width: 1.5; border.color: win.dropping ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.16)
                Column {
                    anchors.centerIn: parent; spacing: 6
                    Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "basket"; size: 22; color: win.dropping ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.55) }
                    Text { text: win.dropping ? "Drop here" : "Drop files into basket"; color: Qt.rgba(1, 1, 1, 0.62); font.family: Theme.font; font.pixelSize: 11 }
                }
            }

            ListView {
                visible: win.model && win.model.count > 0 && !win.dropping
                width: parent.width; height: parent.height - 34
                spacing: 6; clip: true
                model: win.model
                delegate: Rectangle {
                    required property string url
                    readonly property string fname: decodeURIComponent(url.split("/").filter(x => x).pop() || url)
                    width: ListView.view.width; height: 34; radius: 10
                    color: Qt.rgba(1, 1, 1, 0.08)
                    Row {
                        anchors.fill: parent; anchors.leftMargin: 8; anchors.rightMargin: 8; spacing: 8
                        Icon { anchors.verticalCenter: parent.verticalCenter; name: url.endsWith("/") ? "folder" : "document"; size: 14; color: "#58a6ff" }
                        Text { anchors.verticalCenter: parent.verticalCenter; width: parent.width - 42; elide: Text.ElideMiddle; text: fname; color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium }
                    }
                    Drag.active: bdh.active
                    Drag.dragType: Drag.Automatic
                    Drag.supportedActions: Qt.CopyAction
                    Drag.mimeData: ({ "text/uri-list": url })
                    DragHandler { id: bdh; target: null }
                    TapHandler { onTapped: Qt.openUrlExternally(url) }
                }
            }
        }
    }
}
