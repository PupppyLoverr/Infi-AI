import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Pipewire

// Exact getdroppy.app Per-App Audio Mixer Widget:
// Top Speaker (amber active) & Mic toggle circles + Reset circle; vertical list
// of full-width capsule volume sliders with rounded level fill, unity vertical
// tick mark, app icon + label, and live `%` wired to PipeWire via pactl/wpctl.
Item {
    id: mix
    property bool micMode: false
    property var streams: []

    PwObjectTracker { objects: [Pipewire.defaultAudioSink, Pipewire.defaultAudioSource] }
    readonly property var defaultSink: Pipewire.defaultAudioSink
    readonly property var defaultSource: Pipewire.defaultAudioSource

    function refresh() {
        if (listProc.running) return
        listProc.command = [
            "sh", "-c",
            mix.micMode
                ? "pactl list source-outputs 2>/dev/null | awk '/Source Output #/{if(id!=\"\") print id\"|\"app\"|\"vol; id=$3; sub(\"#\",\"\",id); app=\"App\"; vol=100} /Volume:/{if(match($0, /[0-9]+%/)) {v=substr($0,RSTART,RLENGTH-1); vol=v}} /application\\.name =/{gsub(/^.*= \"|\"$/,\"\"); app=$0} END{if(id!=\"\") print id\"|\"app\"|\"vol}'"
                : "pactl list sink-inputs 2>/dev/null | awk '/Sink Input #/{if(id!=\"\") print id\"|\"app\"|\"vol; id=$3; sub(\"#\",\"\",id); app=\"App\"; vol=100} /Volume:/{if(match($0, /[0-9]+%/)) {v=substr($0,RSTART,RLENGTH-1); vol=v}} /application\\.name =/{gsub(/^.*= \"|\"$/,\"\"); app=$0} END{if(id!=\"\") print id\"|\"app\"|\"vol}'"
        ]
        listProc.running = true
    }

    function setStreamVolume(streamObj, pct) {
        const clamped = Math.max(0, Math.min(150, Math.round(pct)))
        const next = []
        for (let i = 0; i < streams.length; i++) {
            const s = streams[i]
            if (s.id === streamObj.id) next.push({ id: s.id, name: s.name, pct: clamped, icon: s.icon, master: s.master })
            else next.push(s)
        }
        streams = next
        if (streamObj.master) {
            const node = mix.micMode ? mix.defaultSource : mix.defaultSink
            if (node && node.audio) {
                node.audio.muted = false
                node.audio.volume = Math.min(1.5, clamped / 100)
            }
        } else {
            Quickshell.execDetached([
                "pactl",
                mix.micMode ? "set-source-output-volume" : "set-sink-input-volume",
                String(streamObj.id),
                clamped + "%"
            ])
        }
    }

    function resetAll() {
        for (let i = 0; i < streams.length; i++) setStreamVolume(streams[i], 100)
    }

    function appIconFor(name) {
        const n = (name || "").toLowerCase()
        if (n.indexOf("firefox") >= 0 || n.indexOf("chromium") >= 0 || n.indexOf("browser") >= 0) return Qt.resolvedUrl("apps/browser.png")
        if (n.indexOf("mpv") >= 0 || n.indexOf("music") >= 0 || n.indexOf("spotify") >= 0) return Qt.resolvedUrl("apps/music.png")
        if (n.indexOf("term") >= 0 || n.indexOf("alacritty") >= 0) return Qt.resolvedUrl("apps/terminal.png")
        if (n.indexOf("files") >= 0 || n.indexOf("finder") >= 0) return Qt.resolvedUrl("apps/files.png")
        return ""
    }

    Component.onCompleted: refresh()
    onVisibleChanged: if (visible) refresh()
    onMicModeChanged: refresh()

    Timer { interval: 2200; running: mix.visible; repeat: true; onTriggered: mix.refresh() }

    Process {
        id: listProc
        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.trim().split("\n").filter(x => x.length > 0)
                const out = []
                for (let i = 0; i < lines.length; i++) {
                    const p = lines[i].split("|")
                    if (p.length >= 3) {
                        const nm = p[1] || "Audio Stream"
                        out.push({
                            id: p[0],
                            name: nm,
                            pct: Math.max(0, Math.min(150, parseInt(p[2]) || 100)),
                            icon: mix.appIconFor(nm),
                            master: false
                        })
                    }
                }
                // Always include Master Output/Input so mixer is genuinely useful even when idle
                const node = mix.micMode ? mix.defaultSource : mix.defaultSink
                const masterPct = node && node.audio ? Math.round((node.audio.muted ? 0 : node.audio.volume) * 100) : 75
                out.push({
                    id: "master",
                    name: mix.micMode ? "System Microphone" : "System Output",
                    pct: masterPct,
                    icon: "",
                    master: true
                })
                mix.streams = out
            }
        }
    }

    Column {
        anchors.fill: parent
        spacing: 10

        // Top toolbar: Speaker (amber active) + Mic left, Reset circle right
        Item {
            width: parent.width
            height: 30
            Row {
                anchors.left: parent.left
                spacing: 8
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: !mix.micMode ? "#4a2f08" : Qt.rgba(1, 1, 1, 0.12)
                    border.width: 1
                    border.color: !mix.micMode ? Qt.rgba(1, 0.62, 0.04, 0.45) : "transparent"
                    Icon { anchors.centerIn: parent; name: "volume"; size: 15; color: !mix.micMode ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.72) }
                    TapHandler { onTapped: mix.micMode = false }
                }
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: mix.micMode ? "#4a2f08" : Qt.rgba(1, 1, 1, 0.12)
                    border.width: 1
                    border.color: mix.micMode ? Qt.rgba(1, 0.62, 0.04, 0.45) : "transparent"
                    Icon { anchors.centerIn: parent; name: "mic"; size: 15; color: mix.micMode ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.72) }
                    TapHandler { onTapped: mix.micMode = true }
                }
            }
            Rectangle {
                anchors.right: parent.right
                width: 30; height: 30; radius: 15
                color: Qt.rgba(1, 1, 1, 0.12)
                Icon { anchors.centerIn: parent; name: "refresh"; size: 14; color: Qt.rgba(1, 1, 1, 0.85) }
                TapHandler { onTapped: mix.resetAll() }
            }
        }

        // Interactive Per-App Capsule Sliders
        ListView {
            width: parent.width
            height: parent.height - 40
            spacing: 7
            clip: true
            model: mix.streams
            delegate: Rectangle {
                id: rowCap
                required property var modelData
                width: ListView.view.width
                height: 38
                radius: 19
                color: "#121316"
                border.width: 1
                border.color: Qt.rgba(1, 1, 1, 0.08)
                clip: true

                // Horizontal level fill bar (100% aligns with the unity marker at ~68% width, boost up to 150%)
                Rectangle {
                    height: parent.height
                    width: parent.width * Math.min(1, Math.max(0, rowCap.modelData.pct / 150))
                    color: Qt.rgba(1, 1, 1, 0.22)
                    Behavior on width { NumberAnimation { duration: 90 } }
                }

                // Droppy crisp vertical 100% unity hairline marker
                Rectangle {
                    x: parent.width * (100 / 150)
                    y: 0
                    width: 1
                    height: parent.height
                    color: Qt.rgba(1, 1, 1, 0.18)
                }

                // App icon + name left
                Row {
                    anchors.left: parent.left; anchors.leftMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 9
                    Rectangle {
                        width: 22; height: 22; radius: 6
                        color: rowCap.modelData.icon !== "" ? "transparent" : Qt.rgba(1, 1, 1, 0.14)
                        Image {
                            visible: rowCap.modelData.icon !== ""
                            anchors.fill: parent
                            source: rowCap.modelData.icon
                            sourceSize: Qt.size(44, 44)
                        }
                        Icon {
                            visible: rowCap.modelData.icon === ""
                            anchors.centerIn: parent
                            name: mix.micMode ? "mic" : "volume"
                            size: 13
                            color: "white"
                        }
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: rowCap.modelData.name
                        color: "white"
                        font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold
                    }
                }

                // Right-aligned live %
                Text {
                    anchors.right: parent.right; anchors.rightMargin: 14
                    anchors.verticalCenter: parent.verticalCenter
                    text: rowCap.modelData.pct + "%"
                    color: Qt.rgba(1, 1, 1, 0.85)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    function setAt(mx) {
                        const pct = (mx / Math.max(1, rowCap.width)) * 150
                        mix.setStreamVolume(rowCap.modelData, pct)
                    }
                    onPressed: mouse => setAt(mouse.x)
                    onPositionChanged: mouse => { if (pressed) setAt(mouse.x) }
                }
            }
        }
    }
}
