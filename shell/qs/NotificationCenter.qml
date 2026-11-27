import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

// Notification Center (macOS: click the clock): every notification still
// open, newest first, grouped by app, each with its actions; dismiss one with
// ×, a whole app with its group's ×, or all with Clear All. A glass column
// on the right, under the menu bar; Escape or a click outside closes it.
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

    Glass {
        id: panel
        anchors { top: parent.top; right: parent.right; bottom: parent.bottom; topMargin: 36; rightMargin: 10; bottomMargin: 92 }
        width: 370; r: 24
        MouseArea { anchors.fill: parent }
        Row {
            id: head
            x: 16; y: 14; width: parent.width - 32; height: 28
            Text { width: parent.width - clearAll.width; anchors.verticalCenter: parent.verticalCenter
                text: "Notifications"; color: Theme.ink; font.family: Theme.font; font.pixelSize: 17; font.weight: Font.Bold }
            Pill { id: clearAll; height: 26; visible: nc.groups.length > 0; text: "Clear All"; onClicked: nc.shell.clearHistory() }
        }
        Text {
            visible: nc.groups.length === 0
            anchors.centerIn: parent
            text: "No Notifications"; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 14
        }
        Flickable {
            anchors { top: head.bottom; left: parent.left; right: parent.right; bottom: parent.bottom; margins: 12; topMargin: 10 }
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
                                text: modelData.app; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: "x"; size: 12; color: Theme.ink2
                                TapHandler { onTapped: for (const note of modelData.notes) note.dismiss() } }
                        }
                        Repeater {
                            model: modelData.notes
                            Rectangle {
                                required property var modelData
                                readonly property var note: modelData
                                width: groupsCol.width; height: body.height + 20; radius: 16
                                color: Theme.fill
                                Column {
                                    id: body
                                    x: 12; y: 10; width: parent.width - 40; spacing: 2
                                    Text { width: parent.width; text: note.summary; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
                                    Text { width: parent.width; visible: text !== ""; text: note.body; wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 12; textFormat: Text.PlainText }
                                    Row { spacing: 6; visible: note.actions.length > 0; topPadding: 4
                                        Repeater { model: note.actions; Pill { required property var modelData; height: 24; text: modelData.text; onClicked: modelData.invoke() } } }
                                }
                                Icon { anchors { top: parent.top; right: parent.right; margins: 10 } name: "x"; size: 11; color: Theme.ink2
                                    TapHandler { onTapped: note.dismiss() } }
                            }
                        }
                    }
                }
            }
        }
    }
}
