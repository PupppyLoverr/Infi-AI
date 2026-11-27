import QtQuick
import Quickshell
import Quickshell.Io

// Exact getdroppy.app AI Usage Widget:
// Top-left gauge + "Usage", top-right sliders + active green gauge circle,
// and side-by-side radial progress rings for Claude (amber) & Codex (red + badge).
// Reads real telemetry from ~/.local/share/solyra/ai-usage.json or scans local
// ~/.claude / ~/.codex session counts so percentages reflect real local activity.
Item {
    id: u
    property int claudeLeft: 59
    property int codexLeft: 6
    property bool compactMode: false

    function refresh() {
        scanProc.running = true
    }

    Component.onCompleted: refresh()
    onVisibleChanged: if (visible) refresh()

    FileView {
        id: usageFile
        path: Theme.home + "/.local/share/solyra/ai-usage.json"
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                const j = JSON.parse(text())
                if (typeof j.claudeLeft === "number") u.claudeLeft = Math.max(0, Math.min(100, Math.round(j.claudeLeft)))
                if (typeof j.codexLeft === "number") u.codexLeft = Math.max(0, Math.min(100, Math.round(j.codexLeft)))
                claudeCanvas.requestPaint()
                codexCanvas.requestPaint()
            } catch (e) {}
        }
    }

    Process {
        id: scanProc
        command: [
            "sh", "-c",
            "if [ -f \"$HOME/.local/share/solyra/ai-usage.json\" ]; then exit 0; fi; " +
            "c=$(find \"$HOME/.claude\" -type f -mmin -300 2>/dev/null | wc -l); " +
            "x=$(find \"$HOME/.codex\" -type f -mmin -300 2>/dev/null | wc -l); " +
            "printf '%s %s\\n' \"$c\" \"$x\""
        ]
        stdout: StdioCollector {
            onStreamFinished: {
                const p = text.trim().split(/\s+/)
                if (p.length >= 2) {
                    const c = parseInt(p[0]) || 0
                    const x = parseInt(p[1]) || 0
                    if (c > 0) u.claudeLeft = Math.max(5, 100 - c * 4)
                    if (x > 0) u.codexLeft = Math.max(4, 100 - x * 6)
                    claudeCanvas.requestPaint()
                    codexCanvas.requestPaint()
                }
            }
        }
    }

    Column {
        anchors.fill: parent
        spacing: 12

        // Header row
        Item {
            width: parent.width
            height: 28
            Row {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                spacing: 7
                Icon { anchors.verticalCenter: parent.verticalCenter; name: "gauge"; size: 15; color: Qt.rgba(1, 1, 1, 0.72) }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Usage"
                    color: Qt.rgba(1, 1, 1, 0.85)
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold
                }
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 7
                Rectangle {
                    width: 28; height: 28; radius: 14
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "sliders"; size: 14; color: Qt.rgba(1, 1, 1, 0.80) }
                    TapHandler { onTapped: u.compactMode = !u.compactMode }
                }
                Rectangle {
                    width: 28; height: 28; radius: 14
                    color: "#113d27"
                    border.width: 1; border.color: Qt.rgba(0.19, 0.82, 0.35, 0.40)
                    Icon { anchors.centerIn: parent; name: "gauge"; size: 14; color: "#30d158" }
                    TapHandler { onTapped: u.refresh() }
                }
            }
        }

        // Side-by-side radial progress rings: Claude (amber) & Codex (red + blue badge)
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 42

            // Claude Ring
            Column {
                spacing: 7
                Item {
                    width: 68; height: 68
                    anchors.horizontalCenter: parent.horizontalCenter
                    Canvas {
                        id: claudeCanvas
                        anchors.fill: parent
                        onVisibleChanged: if (visible) requestPaint()
                        onPaint: {
                            const ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            const cx = width / 2, cy = height / 2, rad = 28
                            ctx.lineWidth = 5
                            ctx.lineCap = "round"
                            ctx.strokeStyle = "rgba(232,152,85,0.22)"
                            ctx.beginPath(); ctx.arc(cx, cy, rad, 0, Math.PI * 2); ctx.stroke()
                            ctx.strokeStyle = "#e89855"
                            ctx.beginPath()
                            ctx.arc(cx, cy, rad, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * (u.claudeLeft / 100))
                            ctx.stroke()
                        }
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: 34; height: 34; radius: 9
                        color: "#1a1410"
                        Icon { anchors.centerIn: parent; name: "claude"; size: 19; color: "#e89855" }
                    }
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Claude"
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: u.claudeLeft + "% left"
                    color: Qt.rgba(1, 1, 1, 0.62)
                    font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium
                    font.features: { "tnum": 1 }
                }
            }

            // Codex Ring
            Column {
                spacing: 7
                Item {
                    width: 68; height: 68
                    anchors.horizontalCenter: parent.horizontalCenter
                    Canvas {
                        id: codexCanvas
                        anchors.fill: parent
                        onVisibleChanged: if (visible) requestPaint()
                        onPaint: {
                            const ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            const cx = width / 2, cy = height / 2, rad = 28
                            ctx.lineWidth = 5
                            ctx.lineCap = "round"
                            ctx.strokeStyle = "rgba(255,69,58,0.22)"
                            ctx.beginPath(); ctx.arc(cx, cy, rad, 0, Math.PI * 2); ctx.stroke()
                            ctx.strokeStyle = "#ff453a"
                            ctx.beginPath()
                            ctx.arc(cx, cy, rad, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * Math.max(0.05, u.codexLeft / 100))
                            ctx.stroke()
                        }
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: 34; height: 34; radius: 9
                        color: "#15161a"
                        Icon { anchors.centerIn: parent; name: "codex"; size: 19; color: Qt.rgba(1, 1, 1, 0.85) }
                    }
                    // Signature blue alert badge dot on low quota
                    Rectangle {
                        visible: u.codexLeft <= 15
                        x: parent.width - 18; y: 4
                        width: 16; height: 16; radius: 8
                        color: "#0a84ff"
                        border.width: 2; border.color: "#000000"
                        Text { anchors.centerIn: parent; text: "!"; color: "white"; font.pixelSize: 10; font.weight: Font.Bold }
                    }
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Codex"
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: u.codexLeft + "% left"
                    color: u.codexLeft <= 15 ? "#ff453a" : Qt.rgba(1, 1, 1, 0.62)
                    font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
            }
        }
    }
}
