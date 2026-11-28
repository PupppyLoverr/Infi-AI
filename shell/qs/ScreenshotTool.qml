import QtQuick
import Quickshell
import Quickshell.Wayland

// Cupertino Screenshot Tool (Super+Shift+5): subtle selection overlay, bottom
// centered glass toolbar (Entire Screen, Window, Selection, Record, Capture),
// real grim/wl-copy capture, and floating thumbnail preview.
Scope {
    id: sc
    required property var shell
    property bool open: false
    // "screen" | "window" | "selection" | "record"
    property string captureMode: "screen"
    property string lastFile: ""

    function toggle(on) { open = (on === undefined) ? !open : !!on }

    function runCapture() {
        open = false
        if (captureMode === "record") {
            if (sc.shell) sc.shell.toggleRecording()
            return
        }
        const stamp = Qt.formatDateTime(new Date(), "yyyy-MM-dd 'at' HH.mm.ss")
        const out = Quickshell.env("HOME") + "/Pictures/Screenshot " + stamp + ".png"
        sc.lastFile = out
        if (captureMode === "selection") {
            Quickshell.execDetached(["sh", "-lc", "mkdir -p \"$HOME/Pictures\" && g=$(slurp) && grim -g \"$g\" \"" + out + "\" && wl-copy < \"" + out + "\""])
        } else {
            Quickshell.execDetached(["sh", "-lc", "mkdir -p \"$HOME/Pictures\" && grim \"" + out + "\" && wl-copy < \"" + out + "\""])
        }
        thumbHide.restart()
    }

    Timer { id: thumbHide; interval: 4500; onTriggered: sc.lastFile = "" }

    PanelWindow {
        visible: sc.open
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: Qt.rgba(0, 0, 0, 0.16)
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-screenshot"
        WlrLayershell.keyboardFocus: sc.open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        Item {
            anchors.fill: parent
            focus: sc.open
            Keys.onEscapePressed: sc.open = false
            Keys.onReturnPressed: sc.runCapture()

            // Selection frame preview when in selection mode
            Rectangle {
                visible: sc.captureMode === "selection"
                anchors.centerIn: parent
                width: Math.min(parent.width * 0.52, 640)
                height: Math.min(parent.height * 0.48, 380)
                color: "transparent"
                border.width: 1
                border.color: Qt.rgba(1, 1, 1, 0.72)
                radius: 4
            }

            // Bottom centered glass toolbar
            Item {
                anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 82 }
                width: barRow.implicitWidth + 24
                height: 52

                Glass { anchors.fill: parent; kind: "dock"; r: 18 }

                Row {
                    id: barRow
                    anchors.centerIn: parent
                    spacing: 6

                    Rectangle {
                        width: 28; height: 28; radius: 14
                        anchors.verticalCenter: parent.verticalCenter
                        color: Qt.rgba(1, 1, 1, 0.12)
                        Icon { anchors.centerIn: parent; name: "close"; size: 13 }
                        MouseArea { anchors.fill: parent; onClicked: sc.open = false }
                    }

                    Repeater {
                        model: [
                            { id: "screen", label: "Entire Screen", icon: "desktop" },
                            { id: "window", label: "Window", icon: "split" },
                            { id: "selection", label: "Selection", icon: "expand" },
                            { id: "record", label: "Record", icon: "mic" }
                        ]
                        Rectangle {
                            required property var modelData
                            width: modelData.id === "screen" ? 116 : 92; height: 34; radius: 11
                            anchors.verticalCenter: parent.verticalCenter
                            color: sc.captureMode === modelData.id ? Qt.rgba(1, 1, 1, 0.22) : "transparent"
                            Row {
                                anchors.centerIn: parent; spacing: 6
                                Icon { anchors.verticalCenter: parent.verticalCenter; name: modelData.icon; size: 14 }
                                Text {
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: modelData.label; color: "white"
                                    font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                                }
                            }
                            MouseArea { anchors.fill: parent; onClicked: sc.captureMode = modelData.id }
                        }
                    }

                    Rectangle {
                        width: 78; height: 32; radius: 16
                        anchors.verticalCenter: parent.verticalCenter
                        color: "#0a84ff"
                        Text {
                            anchors.centerIn: parent
                            text: sc.captureMode === "record" ? "Record" : "Capture"
                            color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
                        }
                        MouseArea { anchors.fill: parent; onClicked: sc.runCapture() }
                    }
                }
            }
        }
    }

    // Floating thumbnail after capture
    PanelWindow {
        visible: sc.lastFile !== "" && !sc.open
        anchors { right: true; bottom: true }
        margins { right: 18; bottom: 82 }
        implicitWidth: 164; implicitHeight: 112
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-screenshot-thumb"

        Glass { anchors.fill: parent; kind: "module"; r: 14 }
        Image {
            anchors { fill: parent; margins: 6 }
            source: sc.lastFile !== "" ? ("file://" + sc.lastFile) : ""
            fillMode: Image.PreserveAspectCrop
        }
        MouseArea {
            anchors.fill: parent
            onClicked: {
                const f = sc.lastFile
                sc.lastFile = ""
                if (f) Quickshell.execDetached(["xdg-open", f])
            }
        }
    }
}
