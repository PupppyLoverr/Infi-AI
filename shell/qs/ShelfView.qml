import QtQuick
import Quickshell
import Quickshell.Widgets

// Exact getdroppy.app File Tray inside the Notch:
// - Top circular toolbar: Archive/Zip, New Folder+, Blue Select-All on left;
//   Shrink/Compress, Floating Basket, Trash/Clear on right.
// - Horizontal rich preview cards: macOS blue folder, real image thumbnails with
//   size & pixel dimensions (`246 KB · 2560x1440`), PDF slide card, live white
//   text preview page, vibrant blue (#0A84FF) selected card highlight, drag-out,
//   folder drops, and pin support.
Item {
    id: v
    property var model
    property var shell
    property bool dropping: false

    readonly property bool anySelected: {
        const _r = v.model ? v.model.rev : 0
        return v.model ? v.model.hasSelection() : false
    }

    Column {
        anchors.fill: parent
        spacing: 10

        // Top circular Droppy toolbar
        Item {
            width: parent.width
            height: 30

            Row {
                anchors.left: parent.left
                spacing: 8
                // Zip / Extract
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "archive"; size: 15; color: Qt.rgba(1, 1, 1, 0.88) }
                    TapHandler { onTapped: if (v.model) v.model.zipSelected() }
                }
                // New Folder (gather selected)
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "folderAdd"; size: 15; color: Qt.rgba(1, 1, 1, 0.88) }
                    TapHandler { onTapped: if (v.model) v.model.groupIntoNewFolder() }
                }
                // Select All (vibrant blue circle with checkmark when active!)
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: v.anySelected ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "check"; size: 14; color: "white" }
                    TapHandler { onTapped: if (v.model) v.model.toggleSelectAll() }
                }
            }

            Row {
                anchors.right: parent.right
                spacing: 8
                // Shrink / Compress photo or PDF
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "shrink"; size: 14; color: Qt.rgba(1, 1, 1, 0.88) }
                    TapHandler { onTapped: if (v.model) v.model.compressSelected() }
                }
                // Detach to Floating Basket
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: v.shell && v.shell.basketOpen ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "basket"; size: 15; color: "white" }
                    TapHandler { onTapped: if (v.shell) v.shell.basketOpen = !v.shell.basketOpen }
                }
                // Trash / Clear Tray
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "trash"; size: 14; color: Qt.rgba(1, 1, 1, 0.85) }
                    TapHandler { onTapped: if (v.model) v.model.clearUnpinned() }
                }
            }
        }

        // Empty / Drop target state
        Rectangle {
            width: parent.width
            height: parent.height - 40
            visible: !v.model || v.model.count === 0 || v.dropping
            radius: 16
            color: Qt.rgba(1, 1, 1, 0.05)
            border.width: 1.5
            border.color: v.dropping ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.16)
            Column {
                anchors.centerIn: parent; spacing: 7
                Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "tray"; size: 24; color: v.dropping ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.58) }
                Text {
                    text: v.dropping ? "Drop to park in File Tray" : "Drop files here — zip, shrink, group or drag out"
                    color: v.dropping ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.56)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                }
            }
        }

        // Horizontal rich preview file cards
        ListView {
            width: parent.width
            height: parent.height - 40
            visible: v.model && v.model.count > 0 && !v.dropping
            orientation: ListView.Horizontal
            spacing: 12
            clip: true
            model: v.model
            delegate: Rectangle {
                id: card
                required property int index
                required property string url
                required property string name
                required property bool isDir
                required property string ext
                required property string sizeLabel
                required property string dims
                required property string snippet
                required property bool pinned
                required property bool selected
                readonly property bool isImg: /^(png|jpg|jpeg|webp|bmp|gif)$/.test(ext)
                readonly property bool isPdf: ext === "pdf"
                readonly property bool isTxt: /^(md|txt|json|qml|rs|sh|lua)$/.test(ext)
                readonly property string subMeta: dims ? (sizeLabel + " · " + dims) : (sizeLabel || (isDir ? "Folder" : ""))

                width: 126; height: ListView.view.height; radius: 16
                color: selected ? "#0a84ff" : "transparent"
                Behavior on color { ColorAnimation { duration: 120 } }

                Column {
                    anchors.fill: parent
                    anchors.margins: 8
                    spacing: 7

                    // Rich preview area
                    Item {
                        width: parent.width
                        height: parent.height - 38

                        // 1) macOS Sky-Blue Folder Icon
                        Item {
                            visible: card.isDir
                            anchors.centerIn: parent
                            width: 68; height: 54
                            Rectangle {
                                x: 0; y: 0; width: 28; height: 14; radius: 4
                                color: "#4aa8f5"
                            }
                            Rectangle {
                                x: 0; y: 8; width: 68; height: 46; radius: 8
                                gradient: Gradient {
                                    GradientStop { position: 0; color: "#76c9ff" }
                                    GradientStop { position: 1; color: "#3b95e8" }
                                }
                                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.28)
                            }
                        }

                        // 2) Live Image Thumbnail
                        ClippingRectangle {
                            visible: card.isImg && !card.isDir
                            anchors.centerIn: parent
                            width: 84; height: 54; radius: 9
                            color: "#1b1d26"
                            Image {
                                anchors.fill: parent
                                source: card.isImg ? card.url : ""
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                                sourceSize: Qt.size(168, 108)
                            }
                            Rectangle {
                                anchors.fill: parent; radius: 9; color: "transparent"
                                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.20)
                            }
                        }

                        // 3) PDF Slide / Document Preview Card
                        Rectangle {
                            visible: card.isPdf && !card.isDir
                            anchors.centerIn: parent
                            width: 82; height: 54; radius: 8
                            gradient: Gradient {
                                GradientStop { position: 0; color: "#27375c" }
                                GradientStop { position: 1; color: "#151e36" }
                            }
                            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.20)
                            Rectangle {
                                anchors.horizontalCenter: parent.horizontalCenter; y: 10
                                width: 24; height: 24; radius: 12
                                gradient: Gradient {
                                    GradientStop { position: 0; color: "#f6d7c5" }
                                    GradientStop { position: 1; color: "#6878a8" }
                                }
                            }
                            Rectangle { anchors.horizontalCenter: parent.horizontalCenter; y: 38; width: 52; height: 2; radius: 1; color: Qt.rgba(1, 1, 1, 0.78) }
                            Rectangle { anchors.horizontalCenter: parent.horizontalCenter; y: 43; width: 34; height: 2; radius: 1; color: Qt.rgba(1, 1, 1, 0.45) }
                        }

                        // 4) Live White Mini Text / Markdown Document Preview
                        Rectangle {
                            visible: card.isTxt && !card.isDir
                            anchors.centerIn: parent
                            width: 56; height: 64; radius: 7
                            color: "#f7f7fa"
                            clip: true
                            Text {
                                anchors.fill: parent; anchors.margins: 5
                                text: card.snippet || "# Notes\n- Ready"
                                color: "#242428"
                                font.family: Theme.mono; font.pixelSize: 5
                                wrapMode: Text.Wrap
                                font.weight: Theme.wBody
                            }
                        }

                        // 5) Generic Document Badge
                        Rectangle {
                            visible: !card.isDir && !card.isImg && !card.isPdf && !card.isTxt
                            anchors.centerIn: parent
                            width: 52; height: 60; radius: 9
                            color: /^(zip|tar|gz|xz|zst)$/.test(card.ext) ? "#d97706" : "#3a4052"
                            Icon { anchors.centerIn: parent; name: /^(zip|tar|gz|xz|zst)$/.test(card.ext) ? "archive" : "document"; size: 22; color: "white" }
                        }

                        // Pin indicator badge on pinned folders/files
                        Rectangle {
                            visible: card.pinned
                            anchors { top: parent.top; right: parent.right; margins: 2 }
                            width: 18; height: 18; radius: 9
                            color: "#ff9f0a"
                            Icon { anchors.centerIn: parent; name: "pin"; size: 10; color: "white" }
                        }
                    }

                    // Filename + live size / dimensions subtitle
                    Column {
                        width: parent.width
                        spacing: 1
                        Text {
                            width: parent.width
                            horizontalAlignment: Text.AlignHCenter
                            elide: Text.ElideMiddle
                            text: card.name
                            color: "white"
                            font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold
                        }
                        Text {
                            width: parent.width
                            horizontalAlignment: Text.AlignHCenter
                            elide: Text.ElideRight
                            text: card.subMeta
                            color: card.selected ? Qt.rgba(1, 1, 1, 0.86) : Qt.rgba(1, 1, 1, 0.56)
                            font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Medium
                        }
                    }
                }

                // Folder Drops: drag files onto a tray folder to move/copy inside it!
                DropArea {
                    anchors.fill: parent
                    enabled: card.isDir
                    onDropped: drop => {
                        const dest = v.model.cleanPath(card.url)
                        for (const u of drop.urls) {
                            const src = v.model.cleanPath(u.toString())
                            Quickshell.execDetached(["sh", "-c", "cp -r \"$1\" \"$2/\"", "sh", src, dest])
                        }
                        v.model.actionToast("Dropped into " + card.name, "Folder updated")
                        v.model.refreshMeta()
                    }
                }

                Drag.active: dh.active
                Drag.dragType: Drag.Automatic
                Drag.supportedActions: Qt.CopyAction
                Drag.mimeData: ({ "text/uri-list": card.url })
                DragHandler { id: dh; target: null }

                TapHandler {
                    onTapped: if (v.model) v.model.toggleSelect(card.index)
                    onDoubleTapped: Qt.openUrlExternally(card.url)
                }
                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: if (v.model) v.model.togglePin(card.index)
                }
            }
        }
    }
}
