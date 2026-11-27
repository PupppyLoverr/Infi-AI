import QtQuick
import Quickshell

// Droppy File Tray inside the pure-black Notch: drop files in, drag them out
// across apps/spaces, click to open, or run quick actions (Zip, Reveal, Clear).
Item {
    id: v
    property var model
    property bool dropping: false

    Rectangle {
        anchors.fill: parent
        visible: v.model.count === 0 || v.dropping
        radius: 16; color: Qt.rgba(1, 1, 1, 0.04)
        border.width: 1.5
        border.color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.18)
        Column {
            anchors.centerIn: parent; spacing: 8
            Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "tray"; size: 26; color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.58) }
            Text {
                text: v.dropping ? "Drop to park on File Tray" : "Drop files here to keep them handy"
                color: v.dropping ? Theme.accent : Qt.rgba(1, 1, 1, 0.56)
                font.family: Theme.font; font.pixelSize: 13
            }
        }
    }

    Column {
        anchors.fill: parent
        visible: v.model.count > 0 && !v.dropping
        spacing: 10

        ListView {
            width: parent.width
            height: parent.height - 34
            orientation: ListView.Horizontal
            spacing: 10
            clip: true
            model: v.model
            delegate: Rectangle {
                required property string url
                readonly property string fname: decodeURIComponent(url.split("/").filter(x => x).pop() || url)
                readonly property string ext: fname.indexOf(".") > 0 ? fname.split(".").pop().toLowerCase() : ""
                width: 96; height: ListView.view.height; radius: 14
                color: "#14151a"
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.09)
                Column {
                    anchors.centerIn: parent; spacing: 8; width: parent.width - 12
                    Rectangle {
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 40; height: 46; radius: 9
                        color: url.endsWith("/") ? "#3b9bf2"
                             : ext === "pdf" ? "#e5484d"
                             : /^(png|jpg|jpeg|webp|svg)$/.test(ext) ? "#34a853"
                             : /^(zip|tar|gz|xz|zst|iso)$/.test(ext) ? "#f59e0b"
                             : "#5e9cff"
                        Icon { anchors.centerIn: parent; name: url.endsWith("/") ? "folder" : /^(png|jpg|jpeg|webp|svg)$/.test(ext) ? "image" : "document"; size: 20; color: "white" }
                    }
                    Text { width: parent.width; horizontalAlignment: Text.AlignHCenter; elide: Text.ElideMiddle; text: fname; color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium }
                }
                Rectangle {
                    anchors { top: parent.top; right: parent.right; margins: 5 }
                    width: 18; height: 18; radius: 9; color: Qt.rgba(1, 1, 1, 0.14)
                    Text { anchors.centerIn: parent; text: "×"; color: Qt.rgba(1, 1, 1, 0.8); font.pixelSize: 12 }
                    TapHandler { onTapped: v.model.removeUrl(url) }
                }
                Drag.active: dh.active
                Drag.dragType: Drag.Automatic
                Drag.supportedActions: Qt.CopyAction
                Drag.mimeData: ({ "text/uri-list": url })
                DragHandler { id: dh; target: null }
                TapHandler { onTapped: Qt.openUrlExternally(url) }
            }
        }

        // Droppy File Tray quick actions strip
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 8
            Pill {
                height: 24; text: "Reveal in Finder"
                onClicked: Quickshell.execDetached(["solyra-files", Theme.home + "/Downloads"])
            }
            Pill {
                height: 24; text: "Zip All"
                onClicked: {
                    const paths = []
                    for (let i = 0; i < v.model.count; i++) paths.push(v.model.get(i).url.replace(/^file:\/\//, ""))
                    if (paths.length) Quickshell.execDetached(["sh", "-c", "zip -j \"$HOME/Downloads/Shelf-$(date +%H%M%S).zip\" \"$@\" && notify-send -a Solyra 'File Tray' 'Archive saved to Downloads'", "sh"].concat(paths))
                }
            }
            Pill {
                height: 24; text: "Clear Tray"
                onClicked: { while (v.model.count > 0) v.model.removeUrl(v.model.get(0).url) }
            }
        }
    }
}
