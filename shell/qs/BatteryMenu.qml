import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.UPower

// The Battery menu (macOS 26/27): "Battery 88%", the power source, Energy
// Mode (Automatic / Low Power / High Power, through power-profiles-daemon:
// balanced / power-saver / performance), what is using significant energy
// (processes above 10% CPU, from ps), and Battery Settings. The bar symbol
// is a battery filled to the charge with a bolt while charging.
StatusMenu {
    id: b
    property var shell
    panelWidth: 340
    panelHeight: body.implicitHeight + 32
    readonly property var dev: UPower.displayDevice
    readonly property int pct: Math.round(dev.percentage * (dev.percentage <= 1 ? 100 : 1))
    readonly property bool charging: dev.state === UPowerDeviceState.Charging || dev.state === UPowerDeviceState.FullyCharged
    readonly property bool onAdapter: !UPower.onBattery

    symbol: Component {
        Item {
            width: 26; height: 14
            Rectangle { x: 0; y: 0; width: 23; height: 14; radius: 4.5; color: "transparent"; border.width: 1.4; border.color: Qt.rgba(1, 1, 1, 0.9) }
            Rectangle { x: 24; y: 4.5; width: 2; height: 5; radius: 1; color: Qt.rgba(1, 1, 1, 0.9) }
            Rectangle { x: 2.5; y: 2.5; height: 9; radius: 2.5; width: Math.max(2, 18 * b.pct / 100); color: b.pct <= 20 && !b.charging ? "#ff453a" : "white" }
            Icon { visible: b.charging; anchors.centerIn: parent; anchors.horizontalCenterOffset: -1; name: "bolt"; size: 11; color: "#1c1c1e" }
        }
    }

    // Energy Mode through power-profiles-daemon.
    property string profile: ""
    Process { id: getProfile; command: ["powerprofilesctl", "get"]; stdout: StdioCollector { onStreamFinished: b.profile = text.trim() } }
    function setProfile(p) { Quickshell.execDetached(["powerprofilesctl", "set", p]); profile = p }
    // Apps using significant energy: above 10% CPU right now.
    property var hogs: []
    Process {
        id: getHogs
        command: ["sh", "-c", "ps -eo pcpu=,comm= --sort=-pcpu | awk '$1 >= 10 {print $2}' | head -4"]
        stdout: StdioCollector { onStreamFinished: b.hogs = text.split("\n").filter(x => x) }
    }
    onOpened: { getProfile.running = true; getHogs.running = true }

    Column {
        id: body
        width: parent.width
        spacing: 6
        Row {
            width: parent.width
            Text { width: parent.width - pctText.width; text: "Battery"; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
            Text { id: pctText; text: b.pct + "%"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 15; font.weight: Theme.wBody;}
        }
        Text { text: "Power Source: " + (b.onAdapter ? "Power Adapter" : "Battery"); color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wBody;}
        Rectangle { width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.14) }
        Text { visible: b.profile !== ""; topPadding: 4; text: "Energy Mode"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
        Repeater {
            model: b.profile !== "" ? [["balanced", "Automatic", "battery"], ["power-saver", "Low Power", "batteryframe"], ["performance", "High Power", "highpower"]] : []
            Item {
                required property var modelData
                width: body.width; height: 36
                Rectangle {
                    id: dot
                    anchors.verticalCenter: parent.verticalCenter
                    width: 28; height: 28; radius: 14
                    color: b.profile === modelData[0] ? Theme.accent : Qt.rgba(1, 1, 1, 0.14)
                    Icon { anchors.centerIn: parent; name: modelData[2]; size: 15 }
                }
                Text { anchors.left: dot.right; anchors.leftMargin: 12; anchors.verticalCenter: parent.verticalCenter; text: modelData[1]; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wBody;}
                TapHandler { onTapped: b.setProfile(modelData[0]) }
            }
        }
        Rectangle { width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.14) }
        Text { topPadding: 2; text: b.hogs.length ? "Using Significant Energy" : "No Apps Using Significant Energy"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
        Repeater {
            model: b.hogs
            Text { required property string modelData; leftPadding: 2; text: modelData; color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Theme.wBody;}
        }
        Rectangle { width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.14) }
        Text {
            text: "Battery Settings…"; color: "white"; font.family: Theme.font; font.pixelSize: 14
            TapHandler { onTapped: { b.open = false; Quickshell.execDetached(["solyra-settings"]) } }
            font.weight: Theme.wBody
        }
    }
}
