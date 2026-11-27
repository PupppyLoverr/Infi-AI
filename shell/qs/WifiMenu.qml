import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io

// The Wi-Fi menu (macOS 26/27): a Wi-Fi switch, the network you are on, other
// networks nearby with their signal and lock, a password field for a secured
// one you have not joined before, and Wi-Fi Settings. Everything through
// NetworkManager (nmcli).
StatusMenu {
    id: w
    property var shell
    icon: "wifi"
    panelWidth: 320
    panelHeight: Math.min(460, body.implicitHeight + 32)

    property bool enabled_: true
    property var nets: []        // [{ ssid, signal, secure, active }]
    property var known: ({})
    property string asking: ""   // SSID whose password is being typed
    property string status: ""
    function refresh() { radioProc.running = true; listProc.running = true; knownProc.running = true }
    onOpened: { status = ""; asking = ""; refresh() }

    Process { id: radioProc; command: ["nmcli", "-t", "radio", "wifi"]; stdout: StdioCollector { onStreamFinished: w.enabled_ = text.trim() === "enabled" } }
    Process {
        id: listProc
        command: ["nmcli", "-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "auto"]
        stdout: StdioCollector {
            onStreamFinished: {
                const seen = {}, out = []
                for (const line of text.split("\n")) {
                    const f = line.split(":")
                    if (f.length < 4 || !f[1] || seen[f[1]]) continue
                    seen[f[1]] = true
                    out.push({ ssid: f[1], signal: parseInt(f[2]) || 0, secure: f[3] !== "" && f[3] !== "--", active: f[0] === "*" })
                }
                out.sort((a, b) => (b.active - a.active) || (b.signal - a.signal))
                w.nets = out
            }
        }
    }
    Process {
        id: knownProc
        command: ["nmcli", "-t", "-f", "NAME,TYPE", "connection", "show"]
        stdout: StdioCollector { onStreamFinished: { const k = {}; for (const l of text.split("\n")) { const f = l.split(":"); if (f[1] && f[1].indexOf("wireless") >= 0) k[f[0]] = true } w.known = k } }
    }
    Process {
        id: joinProc
        onExited: (code) => { w.status = code === 0 ? "" : "Could not join the network."; w.refresh() }
    }
    function join(n, password) {
        status = "Joining " + n.ssid + "…"; asking = ""
        joinProc.command = password ? ["nmcli", "dev", "wifi", "connect", n.ssid, "password", password]
                         : (known[n.ssid] ? ["nmcli", "connection", "up", n.ssid] : ["nmcli", "dev", "wifi", "connect", n.ssid])
        joinProc.running = true
    }

    Column {
        id: body
        width: parent.width
        spacing: 6
        Row {
            width: parent.width; height: 28
            Text { width: parent.width - sw.width; anchors.verticalCenter: parent.verticalCenter; text: "Wi-Fi"; color: "white"; font.family: Theme.font; font.pixelSize: 15; font.weight: Font.Bold }
            // The switch.
            Rectangle {
                id: sw
                anchors.verticalCenter: parent.verticalCenter
                width: 40; height: 24; radius: 12
                color: w.enabled_ ? Theme.accent : Qt.rgba(1, 1, 1, 0.18)
                Rectangle { width: 20; height: 20; radius: 10; y: 2; x: w.enabled_ ? 18 : 2; color: "white"; Behavior on x { NumberAnimation { duration: Theme.ms(160) } } }
                TapHandler { onTapped: { Quickshell.execDetached(["nmcli", "radio", "wifi", w.enabled_ ? "off" : "on"]); w.enabled_ = !w.enabled_; if (w.enabled_) w.refresh() } }
            }
        }
        Text { visible: w.status !== ""; text: w.status; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12 }
        Text { visible: w.enabled_ && w.nets.length === 0; text: "Looking for networks…"; color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 13 }
        Repeater {
            model: w.enabled_ ? w.nets.slice(0, 8) : []
            Column {
                required property var modelData
                required property int index
                width: body.width
                Text {
                    visible: index === 0 || (index === 1 && w.nets[0].active)
                    topPadding: 4; bottomPadding: 2
                    text: modelData.active ? "Known Network" : "Other Networks"
                    color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                }
                Rectangle {
                    width: parent.width; height: 34; radius: 8
                    color: rowHov.hovered ? Qt.rgba(1, 1, 1, 0.10) : "transparent"
                    Rectangle {
                        id: badge
                        x: 4; anchors.verticalCenter: parent.verticalCenter
                        width: 26; height: 26; radius: 13
                        color: modelData.active ? Theme.accent : Qt.rgba(1, 1, 1, 0.14)
                        Icon { anchors.centerIn: parent; name: "wifi"; size: 14; opacity: 0.4 + modelData.signal / 160 }
                    }
                    Text { anchors.left: badge.right; anchors.leftMargin: 10; anchors.verticalCenter: parent.verticalCenter; width: parent.width - 80; elide: Text.ElideRight; text: modelData.ssid; color: "white"; font.family: Theme.font; font.pixelSize: 14 }
                    Icon { visible: modelData.secure; anchors.right: parent.right; anchors.rightMargin: 8; anchors.verticalCenter: parent.verticalCenter; name: "lock"; size: 12; color: Qt.rgba(1, 1, 1, 0.6) }
                    HoverHandler { id: rowHov }
                    TapHandler {
                        onTapped: {
                            if (modelData.active) return
                            if (modelData.secure && !w.known[modelData.ssid]) w.asking = modelData.ssid
                            else w.join(modelData, "")
                        }
                    }
                }
                // Password for a secured network not joined before.
                Rectangle {
                    visible: w.asking === modelData.ssid
                    width: parent.width; height: visible ? 32 : 0; radius: 8
                    color: Qt.rgba(1, 1, 1, 0.12)
                    TextField {
                        anchors.fill: parent; anchors.leftMargin: 10
                        echoMode: TextInput.Password
                        placeholderText: "Password"
                        color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.45)
                        font.family: Theme.font; font.pixelSize: 13
                        background: null
                        onVisibleChanged: if (visible) forceActiveFocus()
                        onAccepted: w.join(modelData, text)
                    }
                }
            }
        }
        Rectangle { width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.14) }
        Text {
            text: "Wi-Fi Settings…"; color: "white"; font.family: Theme.font; font.pixelSize: 14
            TapHandler { onTapped: { w.open = false; Quickshell.execDetached(["sh", "-c", "command -v nm-connection-editor >/dev/null && exec nm-connection-editor || exec solyra-settings"]) } }
        }
    }
}
