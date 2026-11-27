import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland

// Desktop widgets (macOS Sonoma+): a large clock with the date, a month
// calendar and the weather, as glass cards on the desktop under the windows.
// Weather comes from wttr.in (no key, the IP's location), refreshed every 30
// minutes; without a network the card says so. Off in Control Center.
PanelWindow {
    id: w
    property var screenModel
    screen: screenModel
    visible: Theme.widgets
    anchors { top: true; left: true }
    margins { top: 48; left: 24 }
    implicitWidth: 330
    implicitHeight: col.height
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Bottom
    WlrLayershell.namespace: "solyra-widgets"

    // As macOS: the widgets step back (fade) while windows are open on this
    // screen's desktop, and come forward on an empty desktop.
    readonly property var workspace: {
        const m = Hyprland.monitorFor(w.screen)
        return m ? m.activeWorkspace : Hyprland.focusedWorkspace
    }
    readonly property bool windowsOpen: workspace ? workspace.toplevels.values.length > 0 : false

    property date now: new Date()
    Timer { interval: 1000 * (60 - new Date().getSeconds()); running: true; repeat: true; onTriggered: { w.now = new Date(); interval = 60000 } }

    property string temp: ""
    property string cond: ""
    property string place: ""
    property string range: ""
    property bool weatherFailed: false
    Process {
        id: weatherProc
        running: Theme.widgets
        command: ["curl", "-fsS", "--max-time", "8", "https://wttr.in/?format=j1"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const j = JSON.parse(text)
                    const c = j.current_condition[0]
                    w.temp = c.temp_C + "°"
                    w.cond = c.weatherDesc[0].value
                    const a = j.nearest_area && j.nearest_area[0]
                    w.place = a ? a.areaName[0].value : ""
                    const d = j.weather && j.weather[0]
                    w.range = d ? "H:" + d.maxtempC + "°  L:" + d.mintempC + "°" : ""
                    w.weatherFailed = false
                } catch (e) { w.weatherFailed = true }
            }
        }
        onExited: (code) => { if (code !== 0) w.weatherFailed = true }
    }
    Timer { interval: 30 * 60 * 1000; running: Theme.widgets; repeat: true; onTriggered: weatherProc.running = true }

    Column {
        id: col
        width: parent.width
        opacity: w.windowsOpen ? 0.28 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.ms(260); easing.type: Easing.OutCubic } }
        spacing: 14

        // Clock
        Column {
            spacing: 0
            Text { text: Qt.formatDateTime(w.now, "dddd, d MMMM"); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 17; font.weight: Font.DemiBold
                style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.25) }
            Text { text: Qt.formatDateTime(w.now, "H:mm"); color: Qt.rgba(1, 1, 1, 0.92); font.family: Theme.font; font.pixelSize: 76; font.weight: Font.Bold
                style: Text.Raised; styleColor: Qt.rgba(0, 0, 0, 0.25) }
        }

        Row {
            spacing: 14
            // Calendar
            Glass {
                width: 158; height: 158; r: 22
                Column {
                    x: 12; y: 10; width: parent.width - 24; spacing: 4
                    Text { text: Qt.formatDateTime(w.now, "MMMM").toUpperCase(); color: "#ff453a"; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Bold }
                    Grid {
                        columns: 7; columnSpacing: 0; rowSpacing: 1
                        Repeater {
                            model: ["S", "M", "T", "W", "T", "F", "S"]
                            Text { required property string modelData; width: 19; horizontalAlignment: Text.AlignHCenter; text: modelData; color: Theme.ink3; font.family: Theme.font; font.pixelSize: 9; font.weight: Font.DemiBold }
                        }
                        Repeater {
                            model: {
                                const y = w.now.getFullYear(), m = w.now.getMonth()
                                const first = new Date(y, m, 1).getDay(), days = new Date(y, m + 1, 0).getDate()
                                const cells = []
                                for (let i = 0; i < first; i++) cells.push(0)
                                for (let d = 1; d <= days; d++) cells.push(d)
                                return cells
                            }
                            Item {
                                required property int modelData
                                width: 19; height: 17
                                Rectangle { anchors.centerIn: parent; width: 17; height: 17; radius: 8.5; color: "#ff453a"; visible: parent.modelData === w.now.getDate() }
                                Text { anchors.centerIn: parent; visible: parent.modelData > 0; text: parent.modelData; color: parent.modelData === w.now.getDate() ? "white" : Theme.ink; font.family: Theme.font; font.pixelSize: 9; font.weight: Font.Medium }
                            }
                        }
                    }
                }
            }
            // Weather
            Glass {
                width: 158; height: 158; r: 22
                Column {
                    x: 14; y: 12; width: parent.width - 28; spacing: 2
                    Text { width: parent.width; elide: Text.ElideRight; text: w.weatherFailed ? "Weather" : (w.place || "Weather"); color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
                    Text { text: w.weatherFailed ? "—" : (w.temp || "…"); color: Theme.ink; font.family: Theme.font; font.pixelSize: 40; font.weight: Font.Light }
                }
                Column {
                    x: 14; anchors.bottom: parent.bottom; anchors.bottomMargin: 12; width: parent.width - 28; spacing: 1
                    Text { width: parent.width; wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight
                        text: w.weatherFailed ? "Unavailable offline" : w.cond; color: Theme.ink; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium }
                    Text { visible: !w.weatherFailed; text: w.range; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 11 }
                }
            }
        }
    }
}
