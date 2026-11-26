import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.Mpris
import Quickshell.Services.Pipewire
import Quickshell.Services.UPower

// Control Center: a bento of glass modules with real readings only
// (NetworkManager, BlueZ, PipeWire, brightnessctl, MPRIS, UPower), the
// system-wide glass slider and the accessibility switches.
PanelWindow {
    id: cc
    property var shell
    property bool open: false
    visible: open
    anchors { top: true; right: true }
    margins { top: 6; right: 10 }
    implicitWidth: 372
    implicitHeight: grid.height + 24
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-control"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    property string ssid: ""
    property bool wifiOn: false
    property bool btOn: false
    property real bright: -1
    PwObjectTracker { objects: [Pipewire.defaultAudioSink] }
    readonly property var sink: Pipewire.defaultAudioSink
    readonly property var player: Mpris.players.values.length ? Mpris.players.values[0] : null

    function refresh() { wifiProc.running = true; btProc.running = true; brightProc.running = true }
    onOpenChanged: if (open) refresh()
    Process { id: wifiProc; command: ["sh", "-c", "nmcli -t -f WIFI radio 2>/dev/null; nmcli -t -f ACTIVE,SSID dev wifi 2>/dev/null | grep '^yes' | cut -d: -f2"]
        stdout: StdioCollector { onStreamFinished: { const l = text.split("\n"); cc.wifiOn = l[0] === "enabled"; cc.ssid = l[1] || "" } } }
    Process { id: btProc; command: ["sh", "-c", "bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on"]
        stdout: StdioCollector { onStreamFinished: cc.btOn = text.trim() === "on" } }
    Process { id: brightProc; command: ["sh", "-c", "brightnessctl -m 2>/dev/null | cut -d, -f4 | tr -d %"]
        stdout: StdioCollector { onStreamFinished: cc.bright = text.trim() ? parseInt(text) : -1 } }

    Glass {
        anchors.fill: parent; r: 26
        Column {
            id: grid
            x: 12; y: 12
            spacing: 10
            Row {
            spacing: 10
            // Connectivity
            Tile {
                width: 169; height: 150
                Column {
                    anchors.fill: parent; anchors.margins: 12; spacing: 10
                    Toggle { icon: "wifi"; title: "Wi-Fi"; sub: cc.wifiOn ? (cc.ssid || "Not connected") : "Off"; on: cc.wifiOn
                        onClicked: { Quickshell.execDetached(["nmcli", "radio", "wifi", cc.wifiOn ? "off" : "on"]); cc.wifiOn = !cc.wifiOn } }
                    Toggle { icon: "bluetooth"; title: "Bluetooth"; sub: cc.btOn ? "On" : "Off"; on: cc.btOn
                        onClicked: { Quickshell.execDetached(["bluetoothctl", "power", cc.btOn ? "off" : "on"]); cc.btOn = !cc.btOn } }
                    Toggle { icon: "airplay"; title: "Focus"; sub: cc.shell && cc.shell.dnd ? "Do Not Disturb" : "Off"; on: cc.shell && cc.shell.dnd
                        onClicked: cc.shell.dnd = !cc.shell.dnd }
                }
            }
            Column {
                spacing: 10
                Tile { width: 169; height: 70
                    Toggle { anchors.fill: parent; anchors.margins: 12; icon: "focus"; title: "Auto-arrange"; sub: Theme.autoArrange ? "On" : "Off"; on: Theme.autoArrange
                        onClicked: { Theme.autoArrange = !Theme.autoArrange; Theme.save(); cc.shell.applyArrange() } } }
                Row {
                    spacing: 10
                    Tile { width: 80; height: 70; Icon { anchors.centerIn: parent; name: "lock"; size: 22; TapHandler { onTapped: { cc.open = false; cc.shell.lock() } } } }
                    Tile { width: 79; height: 70; Icon { anchors.centerIn: parent; name: "grid"; size: 22; TapHandler { onTapped: { cc.open = false; cc.shell.snap() } } } }
                }
            }
            }
            Slider2 { width: 348; label: "Display"; icon: "sun"; value: cc.bright < 0 ? -1 : cc.bright / 100
                onMoved: v => { cc.bright = v * 100; Quickshell.execDetached(["brightnessctl", "-q", "set", Math.round(v * 100) + "%"]) } }
            Slider2 { width: 348; label: "Sound"; icon: cc.sink && cc.sink.audio.muted ? "volumeMute" : "volume"; value: cc.sink ? cc.sink.audio.volume : -1
                onMoved: v => { if (cc.sink) cc.sink.audio.volume = v } }
            Slider2 { width: 348; label: "Glass  ·  clear → tinted"; icon: "palette"; value: Theme.glass / 100
                onMoved: v => { Theme.glass = Math.round(v * 100); Theme.save() } }
            Tile {
                width: 348; height: 64
                Row {
                    anchors.fill: parent; anchors.margins: 12; spacing: 12
                    Rectangle { width: 40; height: 40; radius: 10; color: Theme.fill
                        Image { anchors.fill: parent; source: cc.player ? cc.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
                        Icon { visible: !cc.player || !cc.player.trackArtUrl; anchors.centerIn: parent; name: "music"; size: 18 } }
                    Column { width: 170; anchors.verticalCenter: parent.verticalCenter
                        Text { width: parent.width; elide: Text.ElideRight; text: cc.player ? (cc.player.trackTitle || "Unknown") : "Not Playing"; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
                        Text { width: parent.width; elide: Text.ElideRight; text: cc.player ? cc.player.trackArtist : ""; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12 } }
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "prev"; size: 18; TapHandler { onTapped: if (cc.player) cc.player.previous() } }
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: cc.player && cc.player.isPlaying ? "pause" : "play"; size: 22; TapHandler { onTapped: if (cc.player) cc.player.togglePlaying() } }
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "next"; size: 18; TapHandler { onTapped: if (cc.player) cc.player.next() } }
                }
            }
            Tile {
                width: 348; height: pillFlow.height + 20
                Flow {
                    id: pillFlow
                    x: 10; y: 10; width: parent.width - 20; spacing: 8
                    Pill { height: 26; text: "Reduce Motion"; on: Theme.reduceMotion; onClicked: { Theme.reduceMotion = !Theme.reduceMotion; Theme.save() } }
                    Pill { height: 26; text: "Reduce Transparency"; on: Theme.reduceTransparency; onClicked: { Theme.reduceTransparency = !Theme.reduceTransparency; Theme.save() } }
                    Pill { height: 26; text: "Borders"; on: Theme.showBorders; onClicked: { Theme.showBorders = !Theme.showBorders; Theme.save() } }
                    Pill { height: 26; text: "Hide Notch"; on: Theme.hideNotch; onClicked: { Theme.hideNotch = !Theme.hideNotch; Theme.save() } }
                }
            }
        }
    }
}
