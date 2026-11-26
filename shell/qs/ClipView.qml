import QtQuick

// Clipboard cards: click to copy again; pin to keep; incognito toggle.
Item {
    id: v
    property var history
    Text {
        visible: v.history.count === 0
        anchors.centerIn: parent
        text: v.history.incognito ? "Incognito: copies are not recorded" : "Copy something and it appears here"
        color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 13
    }
    Pill { anchors { right: parent.right; top: parent.top } z: 2; height: 24; text: v.history.incognito ? "Incognito on" : "Incognito"; on: v.history.incognito; onClicked: v.history.incognito = !v.history.incognito }
    ListView {
        anchors.fill: parent; anchors.topMargin: 30
        orientation: ListView.Horizontal; spacing: 10
        model: v.history
        delegate: Rectangle {
            required property string text
            required property string kind
            required property bool pinned
            required property int index
            width: 150; height: ListView.view.height; radius: 14
            color: kind === "color" ? text : Qt.rgba(1, 1, 1, 0.08)
            Column {
                anchors.fill: parent; anchors.margins: 10; spacing: 4
                Text { visible: kind !== "color"; text: (kind === "link" ? "Link" : "Text") + (pinned ? "  ★" : ""); color: Qt.rgba(1, 1, 1, 0.55); font.family: Theme.font; font.pixelSize: 11 }
                Text { width: parent.width; height: parent.height - 34; text: parent.parent.text; color: kind === "color" ? "#111" : "white"; wrapMode: Text.WrapAnywhere; elide: Text.ElideRight; font.family: Theme.font; font.pixelSize: 12; font.weight: kind === "color" ? Font.Bold : Font.Normal }
                Text { visible: kind !== "color"; text: parent.parent.text.length + " characters"; color: Qt.rgba(1, 1, 1, 0.4); font.family: Theme.font; font.pixelSize: 10 }
            }
            TapHandler { onTapped: v.history.copy(parent.text) }
            TapHandler { acceptedButtons: Qt.RightButton; onTapped: v.history.setProperty(index, "pinned", !pinned) }
        }
    }
}
