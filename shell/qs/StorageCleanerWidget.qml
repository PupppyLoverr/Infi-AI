import QtQuick
import Quickshell
import Quickshell.Io

// Exact getdroppy.app Storage Cleaner Widget:
// Live `df` + `du` storage breakdown, signature 5-row dot-matrix LED storage bar
// (blue Apps, orange Cache, speckled grey System, dark Available), category legend,
// drag-to-uninstall DropArea, and real teal Sparkle Clean + Trash actions.
Item {
    id: sc
    signal toast(string title, string detail)

    property real totalBytes: 1
    property real usedBytes: 0
    property real availBytes: 0
    property real appsBytes: 0
    property real cacheBytes: 0
    property real systemBytes: 0
    property bool scanning: false
    property bool droppingApp: false

    function fmtBytes(b) {
        if (!(b > 0)) return "0 B"
        if (b >= 1e9) return (b / 1e9).toFixed(1) + " GB"
        if (b >= 1e6) return (b / 1e6).toFixed(1) + " MB"
        if (b >= 1e3) return (b / 1e3).toFixed(1) + " kB"
        return Math.round(b) + " B"
    }

    function refresh() {
        if (scanProc.running) return
        scanning = true
        scanProc.running = true
    }

    function cleanCaches() {
        cleanProc.running = true
    }

    function emptyTrash() {
        trashProc.running = true
    }

    Component.onCompleted: refresh()
    onVisibleChanged: if (visible) refresh()

    Process {
        id: scanProc
        command: [
            "sh", "-c",
            "read tot used avail <<EOF\n$(df -B1 / | awk 'NR==2{print $2, $3, $4}')\nEOF\n" +
            "apps=$(du -sb /usr/bin /usr/lib/flatpak \"$HOME/.local/share/flatpak\" 2>/dev/null | awk '{s+=$1} END{print s+0}')\n" +
            "cache=$(du -sb \"$HOME/.cache\" 2>/dev/null | awk '{s+=$1} END{print s+0}')\n" +
            "printf '%s %s %s %s %s\\n' \"$tot\" \"$used\" \"$avail\" \"$apps\" \"$cache\""
        ]
        stdout: StdioCollector {
            onStreamFinished: {
                const p = text.trim().split(/\s+/)
                if (p.length >= 5) {
                    const tot = Math.max(1, parseFloat(p[0]) || 1)
                    const used = Math.max(0, parseFloat(p[1]) || 0)
                    const avail = Math.max(0, parseFloat(p[2]) || 0)
                    const apps = Math.min(used, Math.max(0, parseFloat(p[3]) || 0))
                    const cache = Math.min(Math.max(0, used - apps), Math.max(0, parseFloat(p[4]) || 0))
                    sc.totalBytes = tot
                    sc.usedBytes = used
                    sc.availBytes = avail
                    sc.appsBytes = apps
                    sc.cacheBytes = cache
                    sc.systemBytes = Math.max(0, used - apps - cache)
                    ledCanvas.requestPaint()
                }
                sc.scanning = false
            }
        }
    }

    Process {
        id: cleanProc
        command: [
            "sh", "-c",
            "before=$(du -sb \"$HOME/.cache\" 2>/dev/null | awk '{print $1+0}'); " +
            "rm -rf \"$HOME/.cache/thumbnails\"/* \"$HOME/.cache/fontconfig\"/* /tmp/*.tmp 2>/dev/null || true; " +
            "after=$(du -sb \"$HOME/.cache\" 2>/dev/null | awk '{print $1+0}'); " +
            "freed=$((before - after)); [ $freed -lt 0 ] && freed=0; echo $freed"
        ]
        stdout: StdioCollector {
            onStreamFinished: {
                const freed = parseFloat(text.trim()) || 0
                sc.toast("Cleaned caches", freed > 0 ? ("Freed " + sc.fmtBytes(freed)) : "System already clean")
                sc.refresh()
            }
        }
    }

    Process {
        id: trashProc
        command: [
            "sh", "-c",
            "before=$(du -sb \"$HOME/.local/share/Trash\" 2>/dev/null | awk '{print $1+0}'); " +
            "rm -rf \"$HOME/.local/share/Trash/files\"/* \"$HOME/.local/share/Trash/info\"/* 2>/dev/null || true; " +
            "echo $before"
        ]
        stdout: StdioCollector {
            onStreamFinished: {
                const freed = parseFloat(text.trim()) || 0
                sc.toast("Emptied Trash", freed > 0 ? ("Freed " + sc.fmtBytes(freed)) : "Trash was empty")
                sc.refresh()
            }
        }
    }

    DropArea {
        anchors.fill: parent
        onEntered: sc.droppingApp = true
        onExited: sc.droppingApp = false
        onDropped: drop => {
            sc.droppingApp = false
            if (drop.urls && drop.urls.length > 0) {
                const f = drop.urls[0].toString().replace(/^file:\/\//, "")
                const base = f.split("/").pop()
                Quickshell.execDetached(["sh", "-c", "rm -rf \"$1\"", "sh", f])
                sc.toast("Removed " + base, "Uninstalled from disk")
                sc.refresh()
            }
        }
    }

    Column {
        anchors.fill: parent
        spacing: 14

        // Header: Big used GB + available left; "of X GB" + circular refresh right
        Item {
            width: parent.width
            height: 40
            Column {
                anchors.left: parent.left
                spacing: 2
                Text {
                    text: sc.fmtBytes(sc.usedBytes)
                    color: "white"
                    font.family: Theme.font; font.pixelSize: 22; font.weight: Font.Bold
                    font.features: { "tnum": 1 }
                }
                Text {
                    text: sc.fmtBytes(sc.availBytes) + " available"
                    color: Qt.rgba(1, 1, 1, 0.56)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium
                }
            }
            Row {
                anchors.right: parent.right
                anchors.top: parent.top
                spacing: 10
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "of " + sc.fmtBytes(sc.totalBytes)
                    color: Qt.rgba(1, 1, 1, 0.65)
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold
                    font.features: { "tnum": 1 }
                }
                Rectangle {
                    width: 28; height: 28; radius: 14
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon {
                        anchors.centerIn: parent; name: "refresh"; size: 14; color: "white"
                        rotation: sc.scanning ? 180 : 0
                        Behavior on rotation { NumberAnimation { duration: 400 } }
                    }
                    TapHandler { onTapped: sc.refresh() }
                }
            }
        }

        // Droppy Signature 5-row Dot-Matrix LED Storage Bar
        Rectangle {
            width: parent.width
            height: 36
            radius: 10
            color: "#111215"
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, 0.07)
            Canvas {
                id: ledCanvas
                anchors.fill: parent
                anchors.margins: 5
                onWidthChanged: requestPaint()
                onHeightChanged: requestPaint()
                onPaint: {
                    const ctx = getContext("2d")
                    ctx.clearRect(0, 0, width, height)
                    const rows = 5
                    const step = 6
                    const cols = Math.max(10, Math.floor((width - 2) / step))
                    const dot = 3.5
                    const yOff = (height - rows * step + (step - dot)) / 2
                    const tot = Math.max(1, sc.totalBytes)
                    const appsCols = Math.max(1, Math.round(cols * (sc.appsBytes / tot)))
                    const cacheCols = Math.max(1, Math.round(cols * (sc.cacheBytes / tot)))
                    const usedCols = Math.min(cols, Math.max(appsCols + cacheCols, Math.round(cols * (sc.usedBytes / tot))))

                    for (let c = 0; c < cols; c++) {
                        for (let r = 0; r < rows; r++) {
                            let fill = "rgba(255,255,255,0.12)"
                            if (c < appsCols) {
                                fill = (r === 2 || r === 3) ? "#1f8bff" : "#0a66cc"
                            } else if (c < appsCols + cacheCols) {
                                fill = (r % 2 === 0) ? "#ff9f0a" : "#c77406"
                            } else if (c < usedCols) {
                                // Subtle Droppy organic grey LED luminance variation
                                const hash = ((c * 31 + r * 17) % 11)
                                const alpha = hash > 7 ? 0.84 : (hash > 3 ? 0.54 : 0.30)
                                fill = "rgba(220,224,232," + alpha + ")"
                            }
                            ctx.fillStyle = fill
                            ctx.beginPath()
                            ctx.roundedRect(c * step, yOff + r * step, dot, dot, 1, 1)
                            ctx.fill()
                        }
                    }
                }
            }
        }

        // Category breakdown row: Apps (blue), Cache (orange), System (grey)
        Row {
            width: parent.width
            spacing: 0
            Item {
                width: parent.width / 3; height: 24
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left; anchors.leftMargin: 4
                    spacing: 12
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "grid"; size: 15; color: "#0a84ff" }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: sc.fmtBytes(sc.appsBytes); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
                }
            }
            Item {
                width: parent.width / 3; height: 24
                Row {
                    anchors.centerIn: parent
                    spacing: 12
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "archive"; size: 15; color: "#ff9f0a" }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: sc.fmtBytes(sc.cacheBytes); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
                }
            }
            Item {
                width: parent.width / 3; height: 24
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right; anchors.rightMargin: 4
                    spacing: 12
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "settings"; size: 15; color: Qt.rgba(1, 1, 1, 0.60) }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: sc.fmtBytes(sc.systemBytes); color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium }
                }
            }
        }

        // Bottom row: "Drop an app to uninstall" + teal Sparkle Clean circle + dark Trash circle
        Item {
            width: parent.width
            height: 36
            Text {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                text: sc.droppingApp ? "Release to remove app" : "Drop an app to uninstall"
                color: sc.droppingApp ? "#ff453a" : Qt.rgba(1, 1, 1, 0.62)
                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 10
                Rectangle {
                    width: 36; height: 36; radius: 18
                    color: "#1a4b44"
                    border.width: 1; border.color: Qt.rgba(0.24, 0.91, 0.77, 0.32)
                    Icon { anchors.centerIn: parent; name: "sparkles"; size: 17; color: "#3ce8c5" }
                    TapHandler { onTapped: sc.cleanCaches() }
                }
                Rectangle {
                    width: 36; height: 36; radius: 18
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "trash"; size: 16; color: Qt.rgba(1, 1, 1, 0.80) }
                    TapHandler { onTapped: sc.emptyTrash() }
                }
            }
        }
    }
}
