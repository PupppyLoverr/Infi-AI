import QtQuick
import Quickshell
import Quickshell.Wayland

// Cupertino 200x200 OSD overlay with a 16-segment meter for volume, mute,
// brightness, keyboard backlight, and microphone.
Scope {
    id: osd
    property bool shown: false
    property string kind: "volume"
    property real value: 0.5

    function show(k, v) {
        kind = k || "volume"
        value = Math.max(0, Math.min(1, v))
        shown = true
        hideTimer.restart()
    }

    Timer { id: hideTimer; interval: 1600; onTriggered: osd.shown = false }

    PanelWindow {
        visible: osd.shown
        anchors { bottom: true }
        margins { bottom: 116 }
        implicitWidth: 200; implicitHeight: 200
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-osd"

        Glass { anchors.fill: parent; kind: "module"; r: 22 }

        Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 44
            size: 68
            name: osd.kind === "brightness" ? "sun" : (osd.kind === "mic" ? "mic" : "volume")
        }

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 158
            spacing: 2
            Repeater {
                model: 16
                Rectangle {
                    required property int index
                    width: 8; height: 7; radius: 1.5
                    color: (index + 1) / 16 <= osd.value + 0.01 ? "white" : Qt.rgba(1, 1, 1, 0.20)
                }
            }
        }
    }
}
