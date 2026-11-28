import QtQuick
import Quickshell

// The Focus menu: Do Not Disturb (banners stay quiet; the Notification Center
// still collects them), on until turned off or for an hour, and Night Shift.
StatusMenu {
    id: f
    property var shell
    icon: "focusring"
    panelWidth: 300
    panelHeight: body.implicitHeight + 32
    Timer { id: hour; interval: 3600 * 1000; onTriggered: if (f.shell) f.shell.dnd = false }

    Column {
        id: body
        width: parent.width
        spacing: 8
        Text { text: "Focus"; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
        Repeater {
            model: [["Do Not Disturb", "moon", "dnd"], ["Do Not Disturb for 1 Hour", "clock", "hour"], ["Night Shift", "sun", "night"]]
            Item {
                required property var modelData
                readonly property bool on: modelData[2] === "night" ? Theme.nightShift : (f.shell && f.shell.dnd && (modelData[2] === "dnd" ? !hour.running : hour.running))
                width: body.width; height: 38
                Rectangle {
                    id: dot
                    anchors.verticalCenter: parent.verticalCenter
                    width: 30; height: 30; radius: 15
                    color: parent.on ? (modelData[2] === "night" ? "#ff9f0a" : "#5e5ce6") : Qt.rgba(1, 1, 1, 0.14)
                    Icon { anchors.centerIn: parent; name: modelData[1]; size: 15 }
                }
                Text { anchors.left: dot.right; anchors.leftMargin: 12; anchors.verticalCenter: parent.verticalCenter; text: modelData[0]; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wBody;}
                TapHandler {
                    onTapped: {
                        const k = modelData[2]
                        if (k === "night") { Theme.nightShift = !Theme.nightShift; Theme.save(); return }
                        const turnOn = !parent.on
                        f.shell.dnd = turnOn
                        if (turnOn && k === "hour") hour.restart(); else hour.stop()
                    }
                }
            }
        }
    }
}
