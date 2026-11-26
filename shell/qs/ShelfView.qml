import QtQuick
import Quickshell

// Shelf grid: drop files in; drag them out; click to open; × to remove.
Item {
    id: v
    property var model
    property bool dropping: false
    Rectangle {
        anchors.fill: parent
        visible: v.model.count === 0 || v.dropping
        radius: 18; color: "transparent"
        border.width: 1.5; border.color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.22)
        Column {
            anchors.centerIn: parent; spacing: 8
            Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "tray"; size: 26; color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.55) }
            Text { text: v.dropping ? "Drop to keep it on the shelf" : "Drop files here to keep them handy"; color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.55); font.family: Theme.font; font.pixelSize: 13 }
        }
    }
    ListView {
        anchors.fill: parent
        visible: v.model.count > 0 && !v.dropping
        orientation: ListView.Horizontal
        spacing: 12
        model: v.model
        delegate: Rectangle {
            required property string url
            width: 92; height: 120; radius: 14; color: Qt.rgba(1, 1, 1, 0.07)
            Column {
                anchors.centerIn: parent; spacing: 6
                Icon { anchors.horizontalCenter: parent.horizontalCenter; name: url.endsWith("/") ? "folder" : "document"; size: 40; color: "white" }
                Text { width: 80; horizontalAlignment: Text.AlignHCenter; elide: Text.ElideMiddle; text: decodeURIComponent(url.split("/").filter(x => x).pop() || url); color: "white"; font.family: Theme.font; font.pixelSize: 11 }
            }
            Text { anchors { top: parent.top; right: parent.right; margins: 6 } text: "×"; color: Qt.rgba(1, 1, 1, 0.6); font.pixelSize: 14; TapHandler { onTapped: v.model.removeUrl(url) } }
            Drag.active: dh.active
            Drag.dragType: Drag.Automatic
            Drag.supportedActions: Qt.CopyAction
            Drag.mimeData: ({ "text/uri-list": url })
            DragHandler { id: dh; target: null }
            TapHandler { onTapped: Qt.openUrlExternally(url) }
        }
    }
}
