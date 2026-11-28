import QtQuick
import Quickshell
import Quickshell.Wayland

// Cupertino Session Agent confirmation panels: Log Out, Restart, Shut Down, and
// Power Key dialog with a 60-second automatic countdown and Reopen Windows option.
Scope {
    id: sa
    required property var shell
    // "" | "logout" | "restart" | "shutdown" | "power"
    property string mode: ""
    property int countdown: 60
    property bool reopenWindows: true

    function open(kind) {
        mode = kind || ""
        countdown = 60
        if (mode !== "") tick.restart()
        else tick.stop()
    }

    function execute(kind) {
        const k = kind || mode
        open("")
        if (k === "logout") Quickshell.execDetached(["hyprctl", "dispatch", "exit"])
        else if (k === "restart") Quickshell.execDetached(["systemctl", "reboot"])
        else if (k === "shutdown") Quickshell.execDetached(["systemctl", "poweroff"])
        else if (k === "sleep") Quickshell.execDetached(["systemctl", "suspend"])
    }

    Timer {
        id: tick
        interval: 1000
        repeat: true
        onTriggered: {
            if (sa.mode === "") { stop(); return }
            if (sa.countdown <= 1) sa.execute(sa.mode)
            else sa.countdown -= 1
        }
    }

    PanelWindow {
        visible: sa.mode !== ""
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: Qt.rgba(0, 0, 0, 0.28)
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-session-agent"
        WlrLayershell.keyboardFocus: sa.mode !== "" ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        MouseArea { anchors.fill: parent; onClicked: sa.open("") }

        Item {
            id: card
            anchors.centerIn: parent
            width: 340
            height: sa.mode === "power" ? 210 : 236
            focus: sa.mode !== ""
            Keys.onEscapePressed: sa.open("")
            Keys.onReturnPressed: if (sa.mode !== "power") sa.execute(sa.mode)

            Glass { anchors.fill: parent; kind: "module"; r: 22 }

            Column {
                anchors { fill: parent; margins: 22 }
                spacing: 12

                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: 48; height: 48; radius: 24
                    color: Qt.rgba(1, 1, 1, 0.14)
                    Icon {
                        anchors.centerIn: parent
                        name: sa.mode === "restart" ? "refresh" : (sa.mode === "logout" ? "close" : "power")
                        size: 22
                    }
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    text: sa.mode === "logout"
                        ? "Are you sure you want to log out now?"
                        : sa.mode === "restart"
                        ? "Are you sure you want to restart your computer now?"
                        : sa.mode === "power"
                        ? "Are you sure you want to shut down your computer now?"
                        : "Are you sure you want to shut down your computer now?"
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wTitle
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    text: "If you do nothing, the system will "
                        + (sa.mode === "logout" ? "log out" : sa.mode === "restart" ? "restart" : "shut down")
                        + " automatically in " + sa.countdown + " seconds."
                    color: Qt.rgba(1, 1, 1, 0.68)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                }

                Row {
                    visible: sa.mode !== "power"
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 8
                    Rectangle {
                        width: 16; height: 16; radius: 4
                        color: sa.reopenWindows ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.14)
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.24)
                        Icon { visible: sa.reopenWindows; anchors.centerIn: parent; name: "check"; size: 11 }
                        MouseArea { anchors.fill: parent; onClicked: sa.reopenWindows = !sa.reopenWindows }
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Reopen windows when logging back in"
                        color: Qt.rgba(1, 1, 1, 0.85)
                        font.family: Theme.font; font.pixelSize: 12; font.weight: Theme.wBody
                    }
                }

                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 10
                    Rectangle {
                        width: sa.mode === "power" ? 74 : 126; height: 30; radius: 15
                        color: Qt.rgba(1, 1, 1, 0.14)
                        Text { anchors.centerIn: parent; text: "Cancel"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wBody }
                        MouseArea { anchors.fill: parent; onClicked: sa.open("") }
                    }
                    Rectangle {
                        visible: sa.mode === "power"
                        width: 74; height: 30; radius: 15
                        color: Qt.rgba(1, 1, 1, 0.14)
                        Text { anchors.centerIn: parent; text: "Restart"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wBody }
                        MouseArea { anchors.fill: parent; onClicked: sa.execute("restart") }
                    }
                    Rectangle {
                        width: sa.mode === "power" ? 96 : 126; height: 30; radius: 15
                        color: "#0a84ff"
                        Text {
                            anchors.centerIn: parent
                            text: sa.mode === "logout" ? "Log Out" : sa.mode === "restart" ? "Restart" : "Shut Down"
                            color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
                        }
                        MouseArea { anchors.fill: parent; onClicked: sa.execute(sa.mode === "power" ? "shutdown" : sa.mode) }
                    }
                }
            }
        }
    }
}
