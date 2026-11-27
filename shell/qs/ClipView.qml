import QtQuick
import Quickshell

// Droppy-style Rich Clipboard Manager inside the Dynamic Island:
// category filter pills (Clipboard, Work, Personal), Incognito toggle, rich
// horizontal cards (Text, Link, Color swatch with RGB, Video), and dark
// translucent right-click context menu (Copy, Pin, Favorite, Tag, Delete).
Item {
    id: v
    property var history
    property string activeCategory: "all" // all | work | personal
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

    // Header bar: category pills on the left, Incognito pill on the right.
    Row {
        anchors { left: parent.left; top: parent.top }
        spacing: 6
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
                color: sel ? Qt.rgba(0.04, 0.52, 1.0, 0.24) : Qt.rgba(1, 1, 1, 0.08)
                border.width: 1
                border.color: sel ? Qt.rgba(0.25, 0.65, 1.0, 0.45) : Qt.rgba(1, 1, 1, 0.12)
                Row {
                    id: catRow
                    anchors.centerIn: parent
                    spacing: 6
                    Rectangle { width: 7; height: 7; radius: 3.5; anchors.verticalCenter: parent.verticalCenter; color: modelData.dot }
                    Text {
                        text: modelData.label
                        color: sel ? "white" : Qt.rgba(1, 1, 1, 0.72)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: sel ? Font.DemiBold : Font.Medium
                    }
                }
                TapHandler { onTapped: v.activeCategory = modelData.id }
            }
        }
    }

    Pill {
        anchors { right: parent.right; top: parent.top }
        z: 2; height: 24
        text: v.history && v.history.incognito ? "Incognito on" : "Incognito"
        on: v.history && v.history.incognito
        onClicked: if (v.history) v.history.incognito = !v.history.incognito
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

            width: 158; height: ListView.view.height; radius: 15
            color: isColor ? (v.history ? v.history.normalizeHex(modelData.text) : modelData.text)
                 : isVideo ? "#231a3d"
                 : Qt.rgba(1, 1, 1, 0.08)
            border.width: 1
            border.color: isColor ? Qt.rgba(1, 1, 1, 0.22) : Qt.rgba(1, 1, 1, 0.11)
            clip: true

            // Video card ambient gradient
            Rectangle {
                visible: card.isVideo
                anchors.fill: parent
                gradient: Gradient {
                    GradientStop { position: 0; color: "#5f27cd" }
                    GradientStop { position: 1; color: "#19132e" }
                }
            }

            // Top pill header (Type · age) + Pin/Favorite badge
            Row {
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: 9 }
                spacing: 6
                Rectangle {
                    height: 20; radius: 10
                    width: Math.min(parent.width - (badgePill.visible ? 36 : 0), pillRow.implicitWidth + 14)
                    color: card.isColor ? Qt.rgba(1, 1, 1, 0.88) : Qt.rgba(1, 1, 1, 0.14)
                    Row {
                        id: pillRow
                        anchors.centerIn: parent
                        spacing: 4
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            size: 11
                            name: card.isColor ? "palette" : card.isLink ? "link" : card.isVideo ? "play" : "document"
                            color: card.isColor ? "#1c1c1e" : "#7ee787"
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: (card.isColor ? "Color" : card.isLink ? "Link" : card.isVideo ? "Video" : "Text") + " · " + (v.history ? v.history.ageLabel(modelData.ts) : "now")
                            color: card.isColor ? "#1c1c1e" : "white"
                            font.family: Theme.font; font.pixelSize: 10; font.weight: Font.DemiBold
                        }
                    }
                }
                Rectangle {
                    id: badgePill
                    visible: modelData.pinned || modelData.favorite
                    height: 20; width: 24; radius: 10
                    color: card.isColor ? Qt.rgba(1, 1, 1, 0.88) : Qt.rgba(1, 1, 1, 0.14)
                    Text {
                        anchors.centerIn: parent
                        text: modelData.favorite ? "★" : "●"
                        color: modelData.favorite ? "#ff9f0a" : "#ff453a"
                        font.pixelSize: 10
                    }
                }
            }

            // Center content by card type
            Text {
                visible: card.isColor
                anchors.centerIn: parent
                text: v.history ? v.history.normalizeHex(modelData.text) : modelData.text
                color: card.fg
                font.family: Theme.mono; font.pixelSize: 16; font.weight: Font.Bold
            }

            Rectangle {
                visible: card.isLink
                anchors.centerIn: parent
                width: 34; height: 34; radius: 17
                color: "#0a84ff"
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
                anchors { left: parent.left; right: parent.right; top: parent.top; bottom: footerTxt.top; margins: 10; topMargin: 35; bottomMargin: 6 }
                text: modelData.text
                color: card.fg
                wrapMode: Text.WrapAnywhere
                elide: Text.ElideRight
                font.family: Theme.font; font.pixelSize: 12
            }

            // Footer: RGB for Color, URL/filename for Link/Video, char count for Text
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
                onTapped: {
                    v.menuItemText = modelData.text
                    v.menuPinned = !!modelData.pinned
                    v.menuFav = !!modelData.favorite
                    ctxMenu.anchor.item = card
                    ctxMenu.visible = true
                }
            }
        }
    }

    // Droppy dark translucent right-click context menu
    PopupWindow {
        id: ctxMenu
        anchor.item: v
        anchor.rect.y: 90
        implicitWidth: 196
        implicitHeight: ctxCol.implicitHeight + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 12; dark: true
            Column {
                id: ctxCol
                x: 6; y: 6; width: parent.width - 12; spacing: 2
                Repeater {
                    model: [
                        { label: "Copy", action: "copy", icon: "copy" },
                        { label: v.menuPinned ? "Unpin" : "Pin", action: "pin", icon: "tag" },
                        { label: v.menuFav ? "Remove Favorite" : "Add to Favorites", action: "fav", icon: "star" },
                        { label: "Tag: Work", action: "work", icon: "tag" },
                        { label: "Tag: Personal", action: "personal", icon: "tag" },
                        { label: "Delete", action: "delete", icon: "trash" }
                    ]
                    Rectangle {
                        required property var modelData
                        width: ctxCol.width; height: 26; radius: 7
                        color: mh.hovered ? (modelData.action === "delete" ? "#ff453a" : Theme.accent) : "transparent"
                        Row {
                            anchors.fill: parent; anchors.leftMargin: 8; spacing: 8
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: modelData.icon; size: 13; color: mh.hovered ? "white" : Qt.rgba(1, 1, 1, 0.8) }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: modelData.label
                                color: mh.hovered ? "white" : (modelData.action === "delete" ? "#ff6961" : "white")
                                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
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
