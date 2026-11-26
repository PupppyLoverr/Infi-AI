import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Notifications

// The notification server: glass banners top-right that slide in, expire,
// carry actions, and stay quiet while Focus (Do Not Disturb) is on.
PanelWindow {
    id: n
    property var shell
    anchors { top: true; right: true }
    margins { top: 8; right: 10 }
    implicitWidth: 360
    implicitHeight: Math.max(1, stack.height)
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-notifications"
    visible: popups.length > 0

    // Live notification objects (a ListModel cannot hold QObjects).
    property var popups: []
    function drop(note) { popups = popups.filter(x => x !== note) }
    NotificationServer {
        keepOnReload: false
        bodySupported: true; actionsSupported: true; imageSupported: true; persistenceSupported: true
        onNotification: note => {
            note.tracked = true
            if (n.shell && n.shell.dnd && note.urgency !== NotificationUrgency.Critical) return
            n.popups = [note].concat(n.popups)
            note.closed.connect(() => n.drop(note))
        }
    }
    Column {
        id: stack
        width: parent.width
        spacing: 8
        Repeater {
            model: n.popups
            Glass {
                required property var modelData
                readonly property var note: modelData
                width: stack.width; height: col.height + 24; r: 20
                x: Theme.reduceMotion ? 0 : 0
                Timer { interval: note.expireTimeout > 0 ? note.expireTimeout : 5000; running: true; onTriggered: n.drop(note) }
                Column {
                    id: col
                    x: 14; y: 12; width: parent.width - 28; spacing: 3
                    Row { spacing: 8
                        Image { width: 18; height: 18; source: note.appIcon ? Quickshell.iconPath(note.appIcon, true) : ""; visible: source != "" }
                        Text { text: note.appName || "Notification"; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold } }
                    Text { width: parent.width; text: note.summary; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold }
                    Text { width: parent.width; text: note.body; wrapMode: Text.Wrap; maximumLineCount: 4; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; textFormat: Text.PlainText }
                    Row { spacing: 6; visible: note.actions.length > 0
                        Repeater { model: note.actions; Pill { required property var modelData; height: 26; text: modelData.text; onClicked: modelData.invoke() } } }
                }
                TapHandler { onTapped: { n.drop(note); note.dismiss() } }
            }
        }
    }
}
