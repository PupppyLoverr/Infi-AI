import QtQuick
import Quickshell

// Exact getdroppy.app Rich Clipboard Manager on pure OLED black:
// category pills (Clipboard blue, Work orange, Personal green), Incognito +
// expand button, horizontal rich cards with white header pills & "..." menu
// pills (Text, Link, full-bleed Color swatch with RGB, Video), and dark
// translucent context menu with blue-highlighted Copy.
Item {
    id: v
    property var history
    property string activeCategory: "all"
    property string menuItemText: ""
    property bool menuPinned: false
    property bool menuFav: false

    readonly property var visibleCards: {
        const _r = v.history ? (v.history.rev + v.history.count) : 0
        const out = []
        if (!v.history) return out
        for (let i = 0; i < v.history.count; i++) {
            const it = v.history.get(i)
            if (v.activeCategory === "all" || it.category === v.activeCategory) {
                out.push({
                    text: it.text,
                    kind: it.kind,
                    pinned: it.pinned,
                    favorite: it.favorite,
                    category: it.category,
                    ts: it.ts
                })
            }
        }
        return out
    }

    function openContextMenu(anchorCard, item) {
        v.menuItemText = item.text
        v.menuPinned = !!item.pinned
        v.menuFav = !!item.favorite
        ctxMenu.anchor.item = anchorCard
        ctxMenu.visible = true
    }

    // Top bar: Droppy category filter pills on left, Incognito + expand button on right.
    Row {
        anchors { left: parent.left; top: parent.top }
        spacing: 7
        z: 2
        Repeater {
            model: [
                { id: "all",      label: "Clipboard", dot: "#0a84ff" },
                { id: "work",     label: "Work",      dot: "#ff9f0a" },
                { id: "personal", label: "Personal",  dot: "#30d158" }
            ]
            Rectangle {
                required property var modelData
                readonly property bool sel: v.activeCategory === modelData.id
                height: 24; radius: 12
                width: catRow.implicitWidth + 18
                color: sel ? Qt.rgba(0.04, 0.42, 0.90, 0.34) : Qt.rgba(1, 1, 1, 0.09)
                border.width: 1
                border.color: sel ? Qt.rgba(0.20, 0.60, 1.0, 0.50) : Qt.rgba(1, 1, 1, 0.10)
                Row {
                    id: catRow
                    anchors.centerIn: parent
                    spacing: 6
                    Rectangle { width: 7; height: 7; radius: 3.5; anchors.verticalCenter: parent.verticalCenter; color: modelData.dot }
                    Text {
                        text: modelData.label
                        color: sel ? "#6bb5ff" : Qt.rgba(1, 1, 1, 0.78)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                    }
                }
                TapHandler { onTapped: v.activeCategory = modelData.id }
            }
        }
    }

    Row {
        anchors { right: parent.right; top: parent.top }
        spacing: 6
        z: 2
        Pill {
            height: 24
            text: v.history && v.history.incognito ? "Incognito on" : "Incognito"
            on: v.history && v.history.incognito
            onClicked: if (v.history) v.history.incognito = !v.history.incognito
        }
        Rectangle {
            width: 24; height: 24; radius: 12
            color: Qt.rgba(1, 1, 1, 0.12)
            Icon { anchors.centerIn: parent; name: "expand"; size: 12; color: Qt.rgba(1, 1, 1, 0.85) }
        }
    }

    Text {
        visible: v.visibleCards.length === 0
        anchors.centerIn: parent
        anchors.verticalCenterOffset: 12
        text: v.history && v.history.incognito
            ? "Incognito: copies are not recorded"
            : (v.activeCategory === "all" ? "Copy something and it appears here" : "No items tagged " + v.activeCategory)
        color: Qt.rgba(1, 1, 1, 0.48)
        font.family: Theme.font; font.pixelSize: 13
        font.weight: Theme.wBody
    }

    ListView {
        id: list
        anchors.fill: parent
        anchors.topMargin: 32
        orientation: ListView.Horizontal
        spacing: 10
        clip: true
        model: v.visibleCards
        delegate: Rectangle {
            id: card
            required property var modelData
            readonly property bool isColor: modelData.kind === "color"
            readonly property bool isLink: modelData.kind === "link"
            readonly property bool isVideo: modelData.kind === "video"
            readonly property bool brightSwatch: isColor && v.history && v.history.isBrightHex(modelData.text)
            readonly property color fg: isColor ? (brightSwatch ? "#16161a" : "#ffffff") : "#ffffff"
            readonly property color fgSub: isColor ? (brightSwatch ? Qt.rgba(0.08, 0.08, 0.1, 0.65) : Qt.rgba(1, 1, 1, 0.72)) : Qt.rgba(1, 1, 1, 0.48)

            width: 148; height: ListView.view.height; radius: 15
            color: isColor ? (v.history ? v.history.normalizeHex(modelData.text) : modelData.text)
                 : isVideo ? "#231a3d"
                 : "#111216"
            border.width: 1
            border.color: isColor ? Qt.rgba(1, 1, 1, 0.20) : Qt.rgba(1, 1, 1, 0.09)
            clip: true

            Rectangle {
                visible: card.isVideo
                anchors.fill: parent
                gradient: Gradient {
                    GradientStop { position: 0; color: "#5f27cd" }
                    GradientStop { position: 1; color: "#19132e" }
                }
            }

            // Droppy crisp white header pill + right white "..." / badge pill
            Item {
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: 8 }
                height: 20
                Rectangle {
                    anchors.left: parent.left
                    height: 20; radius: 10
                    width: Math.min(parent.width - 34, pillRow.implicitWidth + 12)
                    color: "#f4f4f7"
                    Row {
                        id: pillRow
                        anchors.centerIn: parent
                        spacing: 4
                        Rectangle {
                            width: 12; height: 12; radius: 3
                            anchors.verticalCenter: parent.verticalCenter
                            color: card.isColor ? "#ff9f43" : card.isLink ? "#0a84ff" : card.isVideo ? "#5856d6" : "#30d158"
                            Icon {
                                anchors.centerIn: parent; size: 8; color: "white"
                                name: card.isColor ? "palette" : card.isLink ? "link" : card.isVideo ? "play" : "document"
                            }
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: (card.isColor ? "Color" : card.isLink ? "Link" : card.isVideo ? "Video" : "Text") + " · " + (v.history ? v.history.ageLabel(modelData.ts) : "now")
                            color: "#1c1c1e"
                            font.family: Theme.font; font.pixelSize: 10; font.weight: Font.DemiBold
                        }
                    }
                }
                Rectangle {
                    anchors.right: parent.right
                    height: 20; radius: 10
                    width: (modelData.pinned || modelData.favorite) ? 34 : 24
                    color: "#f4f4f7"
                    Row {
                        anchors.centerIn: parent
                        spacing: 3
                        Text {
                            visible: modelData.pinned || modelData.favorite
                            text: modelData.favorite ? "★" : "●"
                            color: modelData.favorite ? "#ff9f0a" : "#ff3b30"
                            font.pixelSize: 9
                            font.weight: Theme.wBody
                        }
                        Text {
                            text: "•••"
                            color: "#3a3a3c"
                            font.pixelSize: 8
                            font.weight: Font.Bold
                        }
                    }
                    TapHandler { onTapped: v.openContextMenu(card, modelData) }
                }
            }

            // Center content
            Text {
                visible: card.isColor
                anchors.centerIn: parent
                text: v.history ? v.history.normalizeHex(modelData.text) : modelData.text
                color: card.fg
                font.family: Theme.mono; font.pixelSize: 15; font.weight: Font.Bold
            }

            Rectangle {
                visible: card.isLink
                anchors.centerIn: parent
                width: 34; height: 34; radius: 17
                gradient: Gradient {
                    GradientStop { position: 0; color: "#4aa3ff" }
                    GradientStop { position: 1; color: "#0055d4" }
                }
                Icon { anchors.centerIn: parent; name: "link"; size: 17; color: "white" }
            }

            Rectangle {
                visible: card.isVideo
                anchors.centerIn: parent
                width: 34; height: 34; radius: 17
                color: Qt.rgba(1, 1, 1, 0.22)
                Icon { anchors.centerIn: parent; name: "play"; size: 16; color: "white" }
            }

            Text {
                visible: !card.isColor && !card.isLink && !card.isVideo
                anchors { left: parent.left; right: parent.right; top: parent.top; bottom: footerTxt.top; margins: 10; topMargin: 36; bottomMargin: 6 }
                text: modelData.text
                color: Qt.rgba(1, 1, 1, 0.92)
                wrapMode: Text.WrapAnywhere
                elide: Text.ElideRight
                font.family: Theme.font; font.pixelSize: 12
                font.weight: Theme.wBody
            }

            // Footer
            Text {
                id: footerTxt
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: 9 }
                elide: Text.ElideRight
                horizontalAlignment: card.isColor ? Text.AlignHCenter : Text.AlignLeft
                text: card.isColor ? (v.history ? v.history.rgbLabel(modelData.text) : "")
                    : (card.isLink || card.isVideo) ? modelData.text
                    : (modelData.text.length + " characters")
                color: card.fgSub
                font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Medium
            }

            TapHandler { onTapped: if (v.history) v.history.copy(modelData.text) }
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: v.openContextMenu(card, modelData)
            }
        }
    }

    // Droppy dark translucent context menu
    PopupWindow {
        id: ctxMenu
        anchor.item: v
        anchor.rect.y: 90
        implicitWidth: 204
        implicitHeight: ctxCol.implicitHeight + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 12; dark: true
            Column {
                id: ctxCol
                x: 6; y: 6; width: parent.width - 12; spacing: 2
                Repeater {
                    model: [
                        { label: "Copy", action: "copy", icon: "copy", primary: true },
                        { label: v.menuPinned ? "Unpin" : "Pin", action: "pin", icon: "tag", primary: false },
                        { label: v.menuFav ? "Remove Favorite" : "Add to Favorites", action: "fav", icon: "star", primary: false },
                        { label: "Tag: Work", action: "work", icon: "tag", primary: false },
                        { label: "Tag: Personal", action: "personal", icon: "tag", primary: false },
                        { label: "Delete from Clipboard", action: "delete", icon: "trash", primary: false }
                    ]
                    Rectangle {
                        required property var modelData
                        width: ctxCol.width; height: 26; radius: 7
                        color: mh.hovered ? (modelData.action === "delete" ? "#ff453a" : Theme.accent)
                             : (modelData.primary ? Theme.accent : "transparent")
                        Row {
                            anchors.fill: parent; anchors.leftMargin: 8; spacing: 8
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: modelData.icon; size: 13; color: "white" }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: modelData.label
                                color: (mh.hovered || modelData.primary) ? "white" : (modelData.action === "delete" ? "#ff6961" : "white")
                                font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                            }
                        }
                        HoverHandler { id: mh }
                        TapHandler {
                            onTapped: {
                                ctxMenu.visible = false
                                if (!v.history || !v.menuItemText) return
                                if (modelData.action === "copy") v.history.copy(v.menuItemText)
                                else if (modelData.action === "pin") v.history.togglePin(v.menuItemText)
                                else if (modelData.action === "fav") v.history.toggleFav(v.menuItemText)
                                else if (modelData.action === "work") v.history.setCategory(v.menuItemText, "work")
                                else if (modelData.action === "personal") v.history.setCategory(v.menuItemText, "personal")
                                else if (modelData.action === "delete") v.history.removeItem(v.menuItemText)
                            }
                        }
                    }
                }
            }
        }
    }
}
