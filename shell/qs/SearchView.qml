import QtQuick
import QtQuick.Controls
import Quickshell

// Spotlight inside the Dynamic Island: the field sits where the island was,
// results grow it downwards. White on the island's dark glass.
Item {
    id: v
    property SearchModel model
    readonly property int fieldH: 46
    readonly property int listH: model.filter === "Ask"
        ? Math.min(360, answerText.implicitHeight + 58)
        : (model.results.length ? Math.min(380, list.contentHeight + 14) : 0)
    implicitHeight: fieldH + (listH > 0 ? listH + 6 : 0)
    function focusField() { field.forceActiveFocus() }
    function setText(t) { field.text = t }

    readonly property color ink: "white"
    readonly property color ink2: Qt.rgba(1, 1, 1, 0.62)
    readonly property color ink3: Qt.rgba(1, 1, 1, 0.42)
    readonly property color fill: Qt.rgba(1, 1, 1, 0.10)

    Row {
        id: top
        width: parent.width; height: v.fieldH
        spacing: 10
        Icon { anchors.verticalCenter: parent.verticalCenter; name: "search"; size: 19; color: v.ink2 }
        TextField {
            id: field
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - 29 - pills.width - 20
            placeholderText: "Search or Ask"
            color: v.ink; placeholderTextColor: v.ink3
            selectionColor: Theme.accent; selectedTextColor: "white"
            font.family: Theme.font; font.pixelSize: 19
            background: null
            onTextChanged: v.model.query = text
            Keys.onPressed: ev => {
                if (ev.key === Qt.Key_Escape) { v.model.done(); ev.accepted = true }
                else if (ev.key === Qt.Key_Down) { v.model.move(1); list.positionViewAtIndex(v.model.sel, ListView.Contain); ev.accepted = true }
                else if (ev.key === Qt.Key_Up) { v.model.move(-1); list.positionViewAtIndex(v.model.sel, ListView.Contain); ev.accepted = true }
                else if (ev.key === Qt.Key_Tab) { v.model.cycleFilter(); ev.accepted = true }
                else if (ev.key === Qt.Key_Return || ev.key === Qt.Key_Enter) { v.model.activate(); ev.accepted = true }
            }
        }
        Row {
            id: pills
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            Repeater {
                model: ["Apps", "Files", "Actions", "Ask"]
                Rectangle {
                    required property string modelData
                    height: 24; width: lbl.implicitWidth + 18; radius: 12
                    color: v.model.filter === modelData ? Theme.accent : v.fill
                    Text { id: lbl; anchors.centerIn: parent; text: modelData; color: v.model.filter === modelData ? "white" : v.ink2; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold }
                    TapHandler { onTapped: { v.model.filter = v.model.filter === modelData ? "All" : modelData; field.forceActiveFocus() } }
                }
            }
        }
    }
    Rectangle {
        visible: v.listH > 0
        anchors.top: top.bottom
        width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.10)
    }

    // Ask: the streamed answer.
    Column {
        visible: v.model.filter === "Ask"
        anchors.top: top.bottom; anchors.topMargin: 10
        width: parent.width; spacing: 10
        Row {
            spacing: 6
            Repeater {
                model: [["claude", "Claude"], ["codex", "ChatGPT"]]
                Rectangle {
                    required property var modelData
                    height: 24; width: pl.implicitWidth + 20; radius: 12
                    color: v.model.provider === modelData[0] ? Qt.rgba(1, 1, 1, 0.22) : v.fill
                    Text { id: pl; anchors.centerIn: parent; text: modelData[1]; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium }
                    TapHandler { onTapped: v.model.provider = modelData[0] }
                }
            }
            Text { anchors.verticalCenter: parent.verticalCenter; leftPadding: 8; text: v.model.asking ? "Thinking…" : (v.model.answer ? "" : "Press Return to ask"); color: v.ink2; font.family: Theme.font; font.pixelSize: 12 }
        }
        Text { id: answerText; width: parent.width; wrapMode: Text.Wrap; text: v.model.answer; color: v.ink; font.family: Theme.font; font.pixelSize: 14; textFormat: Text.MarkdownText }
    }

    ListView {
        id: list
        visible: v.model.filter !== "Ask" && v.listH > 0
        anchors.top: top.bottom; anchors.topMargin: 7
        width: parent.width; height: v.listH - 7
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: v.model.results
        currentIndex: v.model.sel
        delegate: Column {
            required property var modelData
            required property int index
            width: list.width
            Text {
                visible: index === 0 || v.model.results[index - 1].group !== modelData.group
                height: visible ? 22 : 0
                leftPadding: 10; topPadding: 4
                text: modelData.group; color: v.ink3; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold
            }
            Rectangle {
                width: parent.width; height: modelData.group === "Top Hit" ? 50 : 40; radius: 12
                color: index === v.model.sel ? Qt.rgba(1, 1, 1, 0.16) : "transparent"
                Row {
                    anchors.fill: parent; anchors.leftMargin: 10; spacing: 11
                    Image {
                        visible: !!modelData.entry
                        anchors.verticalCenter: parent.verticalCenter
                        width: modelData.group === "Top Hit" ? 36 : 28; height: width
                        sourceSize: Qt.size(64, 64)
                        source: {
                            const e = modelData.entry
                            if (!e) return ""
                            const own = { "solyra-term": "terminal", "solyra-files": "files", "solyra-settings": "settings", "firefox": "browser", "mpv": "music" }
                            if (own[e.id]) return Qt.resolvedUrl("apps/" + own[e.id] + ".png")
                            const p = e.icon ? Quickshell.iconPath(e.icon, true) : ""
                            return p !== "" ? p : Qt.resolvedUrl("apps/overview.png")
                        }
                    }
                    Rectangle {
                        visible: !modelData.entry
                        anchors.verticalCenter: parent.verticalCenter
                        width: 28; height: 28; radius: 8; color: v.fill
                        Icon { anchors.centerIn: parent; name: modelData.icon || "info"; size: 16; color: "white" }
                    }
                    Column {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width - 70
                        Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: v.ink; font.family: Theme.font; font.pixelSize: modelData.group === "Top Hit" ? 15 : 13; font.weight: Font.Medium }
                        Text { width: parent.width; elide: Text.ElideMiddle; text: modelData.sub; color: v.ink2; font.family: Theme.font; font.pixelSize: 11 }
                    }
                }
                HoverHandler { onHoveredChanged: if (hovered) v.model.sel = index }
                TapHandler { onTapped: v.model.activate(index) }
            }
        }
    }
}
