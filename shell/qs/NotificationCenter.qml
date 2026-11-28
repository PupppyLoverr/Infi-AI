import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

// Notification Center (macOS 26/27: click the date): no panel. Notifications
// still open float down the right as frosted cards, newest first, grouped by
// app with their actions (× one, × an app's, Clear All). With none, "No
// recent notifications" floats on the desktop with an "Edit Widgets" glass
// capsule under it (it shows or hides the desktop widgets). Escape or a
// click outside closes it.
PanelWindow {
    id: nc
    property var shell
    visible: shell && shell.centerOpen
    anchors { top: true; bottom: true; left: true; right: true }
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-notifcenter"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    function close() { shell.centerOpen = false }
    // Apps in order of their newest notification.
    readonly property var groups: {
        const order = [], by = {}
        for (const note of (shell ? shell.history : [])) {
            const app = note.appName || "Notifications"
            if (!by[app]) { by[app] = []; order.push(app) }
            by[app].push(note)
        }
        return order.map(app => ({ app: app, notes: by[app] }))
    }

    MouseArea { anchors.fill: parent; onClicked: nc.close() }
    Item { focus: nc.visible; Keys.onEscapePressed: nc.close() }

    Item {
        id: panel
        anchors { top: parent.top; right: parent.right; bottom: parent.bottom; topMargin: 40; rightMargin: 12; bottomMargin: 92 }
        width: 370

        // Nothing to show: the words float on the desktop, as macOS's do.
        Column {
            visible: nc.groups.length === 0
            anchors.horizontalCenter: parent.horizontalCenter
            y: 36; spacing: 26
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "No recent notifications"; color: "white"
                font.family: Theme.font; font.pixelSize: 17; font.weight: Font.DemiBold
                style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.25)
            }
            Glass {
                anchors.horizontalCenter: parent.horizontalCenter
                r: 16; kind: "clear"; shadow: false
                width: ew.implicitWidth + 32; height: 32
                Text { id: ew; anchors.centerIn: parent; text: "Edit Widgets"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
                TapHandler { onTapped: { Theme.widgets = !Theme.widgets; Theme.save() } }
            }
        }
        Row {
            id: head
            visible: nc.groups.length > 0
            width: parent.width; height: 30
            Text { width: parent.width - clearAll.width; anchors.verticalCenter: parent.verticalCenter
                text: "Notifications"; color: "white"; font.family: Theme.font; font.pixelSize: 17; font.weight: Font.Bold
                style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.25) }
            Glass { id: clearAll; r: 14; kind: "clear"; shadow: false; width: ca.implicitWidth + 26; height: 28
                Text { id: ca; anchors.centerIn: parent; text: "Clear All"; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium }
                TapHandler { onTapped: nc.shell.clearHistory() } }
        }
        Flickable {
            visible: nc.groups.length > 0
            anchors { top: head.bottom; left: parent.left; right: parent.right; bottom: parent.bottom; topMargin: 10 }
            contentHeight: groupsCol.height
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            Column {
                id: groupsCol
                width: parent.width
                spacing: 14
                Repeater {
                    model: nc.groups
                    Column {
                        required property var modelData
                        width: groupsCol.width
                        spacing: 6
                        Row {
                            width: parent.width; height: 20
                            Text { width: parent.width - 20; anchors.verticalCenter: parent.verticalCenter
                                text: modelData.app; color: Qt.rgba(1, 1, 1, 0.7); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "x"; size: 12; color: Qt.rgba(1, 1, 1, 0.7)
                                TapHandler { onTapped: { for (const note of modelData.notes) note.dismiss() } } }
                        }
                        Repeater {
                            model: modelData.notes
                            Glass {
                                required property var modelData
                                readonly property var note: modelData
                                width: groupsCol.width; height: body.height + 24; r: 20
                                kind: "frosted"; shadow: false
                                Column {
                                    id: body
                                    x: 14; y: 12; width: parent.width - 44; spacing: 2
                                    Text { width: parent.width; text: note.summary; elide: Text.ElideRight; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
                                    Text { width: parent.width; visible: text !== ""; text: note.body; wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight; color: "white"; font.family: Theme.font; font.pixelSize: 12; textFormat: Text.PlainText; font.weight: Theme.wBody;}
                                    Row { spacing: 6; visible: note.actions.length > 0; topPadding: 4
                                        Repeater { model: note.actions; Pill { required property var modelData; height: 24; text: modelData.text; onClicked: modelData.invoke() } } }
                                }
                                Icon { anchors { top: parent.top; right: parent.right; margins: 10 } name: "x"; size: 11; color: Qt.rgba(1, 1, 1, 0.7)
                                    TapHandler { onTapped: note.dismiss() } }
                            }
                        }
                    }
                }
            }
        }
    }
}
