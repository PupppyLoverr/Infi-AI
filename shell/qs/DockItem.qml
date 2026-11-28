import QtQuick
import QtQuick.Effects
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import Quickshell.Widgets

// macOS 27 Dock item: spring-damped Gaussian magnification, per-icon soft drop
// shadow onto the Liquid Glass shelf, luminous running/background indicator
// dots, Liquid Glass hover tooltip pill with downward caret, interactive
// Downloads folder stack popover, and expanded right-click Dock context menu.
Item {
    id: it
    property var item
    property var dock
    property real centre
    readonly property bool sep: item.kind === "sep"
    readonly property var wins: item.kind === "app" ? dock.windowsOf(item.appId) : []
    readonly property real scaleF: {
        if (sep || dock.mx < 0 || !Theme.dockMagnify || Theme.reduceMotion) return 1
        const d = (dock.mx - centre) / (dock.cell + dock.gap)
        return 1 + 0.56 * Math.exp(-(d * d) / (2 * 1.35 * 1.35))
    }
    width: sep ? 9 : dock.cell * scaleF
    height: sep ? 48 : dock.cell * scaleF
    Behavior on width {
        enabled: !Theme.reduceMotion
        SpringAnimation { spring: 4.6; damping: 0.44; epsilon: 0.2 }
    }
    Behavior on height {
        enabled: !Theme.reduceMotion
        SpringAnimation { spring: 4.6; damping: 0.44; epsilon: 0.2 }
    }

    // Tapered hairline glass divider
    Rectangle {
        visible: it.sep
        anchors.centerIn: parent
        width: 1; height: 34
        gradient: Gradient {
            GradientStop { position: 0.0; color: "transparent" }
            GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, 0.34) }
            GradientStop { position: 1.0; color: "transparent" }
        }
    }

    Item {
        id: iconBox
        visible: !it.sep
        width: parent.width; height: parent.height
        y: bounce.offset
        Image {
            id: art
            anchors.fill: parent
            sourceSize: Qt.size(128, 128)
            visible: false
            source: {
                const k = it.item.kind
                if (k === "downloads" || k === "trash") return Qt.resolvedUrl("apps/" + k + ".png")
                const known = { "solyra-files": "files", "firefox": "browser", "solyra-term": "terminal", "org.gnome.TextEditor": "editor", "mpv": "music", "solyra-settings": "settings" }
                const key = it.item.appId || it.item.id
                if (known[key]) return Qt.resolvedUrl("apps/" + known[key] + ".png")
                const e = it.item.entry
                const p = e && e.icon ? Quickshell.iconPath(e.icon, true) : ""
                return p !== "" ? p : Qt.resolvedUrl("apps/overview.png")
            }
        }
        // Soft per-icon drop shadow onto the Liquid Glass shelf
        MultiEffect {
            anchors.fill: art
            source: art
            shadowEnabled: true
            shadowColor: Qt.rgba(0, 0, 0, 0.42)
            shadowBlur: 0.35
            shadowVerticalOffset: 3
            shadowHorizontalOffset: 0
        }
    }

    QtObject { id: bounce; property real offset: 0 }
    SequentialAnimation {
        id: bounceAnim
        NumberAnimation { target: bounce; property: "offset"; to: -18; duration: Theme.ms(180); easing.type: Easing.OutQuad }
        NumberAnimation { target: bounce; property: "offset"; to: 0; duration: Theme.ms(220); easing.type: Easing.OutBounce }
    }

    // Luminous running indicator dot (solid = running window, hollow = background tray app)
    Item {
        visible: it.wins.length > 0 || it.item.kind === "bg"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.bottom
        anchors.topMargin: 3
        width: 9; height: 9
        Rectangle {
            visible: it.item.kind !== "bg"
            anchors.centerIn: parent
            width: 8; height: 8; radius: 4
            color: Theme.dark ? Qt.rgba(1, 1, 1, 0.22) : Qt.rgba(0, 0, 0, 0.14)
        }
        Rectangle {
            anchors.centerIn: parent
            width: 5; height: 5; radius: 2.5
            color: it.item.kind === "bg" ? "transparent" : (Theme.dark ? Qt.rgba(1, 1, 1, 0.94) : Qt.rgba(0.1, 0.1, 0.12, 0.88))
            border.width: it.item.kind === "bg" ? 1.2 : 0
            border.color: Theme.dark ? Qt.rgba(1, 1, 1, 0.9) : Qt.rgba(0.1, 0.1, 0.12, 0.85)
        }
    }

    // macOS 27 Liquid Glass hover tooltip pill with downward caret
    readonly property bool forcedHover: dock.forcedMx >= 0 && Math.abs(dock.forcedMx - centre) <= (dock.cell + dock.gap) / 2
    Item {
        visible: (hover.hovered || it.forcedHover) && !it.sep && !downloadsPop.visible
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.top
        anchors.bottomMargin: 10
        width: tip.implicitWidth + 24
        height: 34
        Glass {
            kind: "module"
            anchors.top: parent.top
            anchors.horizontalCenter: parent.horizontalCenter
            width: parent.width; height: 27; r: 13.5
            Text {
                id: tip
                anchors.centerIn: parent
                color: Theme.ink
                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
                text: it.item.kind === "downloads" ? "Downloads"
                    : it.item.kind === "trash" ? "Trash"
                    : it.item.kind === "min" ? (it.item.title || "Window")
                    : (it.item.entry ? it.item.entry.name : (it.dock.bundled[it.item.id] || it.item.id)) + (it.item.kind === "bg" ? " (background)" : "")
            }
        }
        Shape {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            width: 12; height: 7
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeWidth: 0
                fillColor: Theme.dark ? Qt.rgba(0.16, 0.17, 0.21, 0.88) : Qt.rgba(0.96, 0.97, 0.99, 0.92)
                startX: 0; startY: 0
                PathLine { x: 12; y: 0 }
                PathLine { x: 6; y: 7 }
                PathLine { x: 0; y: 0 }
            }
        }
    }

    // Interactive Downloads Folder Stack Popover (macOS 27 Liquid Glass 3x2 grid)
    property var downloadFiles: []
    function openDownloadsStack() {
        downloadsPop.visible = !downloadsPop.visible
        if (downloadsPop.visible) {
            dlProc.running = false
            dlProc.running = true
        }
    }
    Connections {
        target: it.dock
        function onDownloadsStackToggled() {
            if (it.item.kind === "downloads") it.openDownloadsStack()
        }
    }
    Process {
        id: dlProc
        command: [
            "sh", "-c",
            "d=\"$HOME/Downloads\"; [ -d \"$d\" ] || exit 0; ls -1t \"$d\" 2>/dev/null | head -n 6 | while IFS= read -r f; do [ -d \"$d/$f\" ] && printf 'D\\t%s\\n' \"$f\" || printf 'F\\t%s\\n' \"$f\"; done"
        ]
        stdout: StdioCollector {
            onStreamFinished: {
                const lines = (text || "").split("\n").filter(l => l.indexOf("\t") > 0)
                it.downloadFiles = lines.map(l => {
                    const idx = l.indexOf("\t")
                    const kind = l.slice(0, idx)
                    const name = l.slice(idx + 1)
                    const ext = name.indexOf(".") > 0 ? name.split(".").pop().toLowerCase() : ""
                    return { isDir: kind === "D", name: name, ext: ext, fullPath: Theme.home + "/Downloads/" + name }
                })
            }
        }
    }
    PopupWindow {
        id: downloadsPop
        anchor.item: it
        anchor.rect.x: -130
        anchor.rect.y: -278
        implicitWidth: 312
        implicitHeight: 268
        color: "transparent"
        visible: false
        Glass {
            anchors.fill: parent; r: 20
            Column {
                anchors.fill: parent; anchors.margins: 14; spacing: 10
                Item {
                    width: parent.width; height: 20
                    Text {
                        anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                        text: "Downloads"; color: Theme.ink; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold
                    }
                    Text {
                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                        text: it.downloadFiles.length + (it.downloadFiles.length === 1 ? " item" : " items")
                        color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12
                    }
                }
                Item {
                    width: parent.width; height: 170
                    Text {
                        visible: it.downloadFiles.length === 0
                        anchors.centerIn: parent
                        text: "Downloads folder is empty"
                        color: Theme.ink2; font.family: Theme.font; font.pixelSize: 13
                    }
                    Grid {
                        visible: it.downloadFiles.length > 0
                        anchors.fill: parent
                        columns: 3; spacing: 8
                        Repeater {
                            model: it.downloadFiles
                            Rectangle {
                                required property var modelData
                                width: 89; height: 80; radius: 12
                                color: tileHov.hovered ? Qt.rgba(1, 1, 1, Theme.dark ? 0.16 : 0.32) : Qt.rgba(1, 1, 1, Theme.dark ? 0.08 : 0.22)
                                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.08)
                                Column {
                                    anchors.centerIn: parent; width: parent.width - 10; spacing: 6
                                    Rectangle {
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        width: 30; height: 34; radius: 7
                                        color: modelData.isDir ? "#3b9bf2"
                                             : modelData.ext === "pdf" ? "#e5484d"
                                             : /^(png|jpg|jpeg|webp|svg)$/.test(modelData.ext) ? "#34a853"
                                             : /^(zip|tar|gz|xz|zst|appimage|iso)$/.test(modelData.ext) ? "#f59e0b"
                                             : "#5e9cff"
                                        Icon {
                                            anchors.centerIn: parent; size: 16; color: "white"
                                            name: modelData.isDir ? "folder"
                                                : /^(png|jpg|jpeg|webp|svg)$/.test(modelData.ext) ? "image"
                                                : "document"
                                        }
                                    }
                                    Text {
                                        width: parent.width; horizontalAlignment: Text.AlignHCenter; elide: Text.ElideRight
                                        text: modelData.name
                                        color: Theme.ink; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium
                                    }
                                }
                                HoverHandler { id: tileHov }
                                TapHandler {
                                    onTapped: {
                                        downloadsPop.visible = false
                                        Quickshell.execDetached(["xdg-open", modelData.fullPath])
                                    }
                                }
                            }
                        }
                    }
                }
                Rectangle { width: parent.width; height: 1; color: Qt.rgba(1, 1, 1, 0.12) }
                Rectangle {
                    width: parent.width; height: 28; radius: 9
                    color: footHov.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: "Open in Finder ›"
                        color: Theme.ink; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                    }
                    HoverHandler { id: footHov }
                    TapHandler {
                        onTapped: {
                            downloadsPop.visible = false
                            Quickshell.execDetached(["solyra-files", Theme.home + "/Downloads"])
                        }
                    }
                }
            }
        }
    }

    HoverHandler { id: hover }
    TapHandler {
        enabled: !it.sep
        onTapped: {
            const k = it.item.kind
            if (k === "downloads") it.openDownloadsStack()
            else if (k === "trash") Quickshell.execDetached(["xdg-open", "trash:///"])
            else if (k === "bg") { if (it.item.tray) it.item.tray.activate() }
            else if (k === "min") it.dock.restore(it.item.address)
            else if (it.wins.length) it.wins[0].activate()
            else if (it.item.entry) { bounceAnim.restart(); it.item.entry.execute() }
            else { bounceAnim.restart(); Quickshell.execDetached([it.item.id]) }
        }
    }
    TapHandler {
        enabled: !it.sep && it.item.kind !== "min"
        acceptedButtons: Qt.RightButton
        onTapped: ctx.visible = true
    }

    // Expanded macOS 27 Dock right-click context menu (App + Dock settings)
    PopupWindow {
        id: ctx
        anchor.item: it
        anchor.rect.y: -185
        implicitWidth: 236; implicitHeight: menuCol.implicitHeight + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 12
            Column {
                id: menuCol
                x: 6; y: 6; width: parent.width - 12; spacing: 2
                Repeater {
                    model: {
                        const base = it.item.kind === "downloads" ? [["Open Downloads in Finder", "open_dl"]]
                            : it.item.kind === "trash" ? [["Open Trash", "open_trash"]]
                            : it.item.kind === "bg" ? [["Open", "open"], ["Stop Running in Background", "stop"]]
                            : it.wins.length ? [["Show All Windows", "show"], ["Quit", "quit"]]
                            : [["Open", "open"]]
                        return base.concat([
                            ["-", "sep"],
                            [Theme.dockMagnify ? "Turn Magnification Off" : "Turn Magnification On", "toggle_mag"],
                            ["Dock & Appearance Settings…", "settings"]
                        ])
                    }
                    Item {
                        required property var modelData
                        width: menuCol.width
                        height: modelData[1] === "sep" ? 7 : 26
                        Rectangle {
                            visible: modelData[1] === "sep"
                            anchors.verticalCenter: parent.verticalCenter
                            x: 8; width: parent.width - 16; height: 1
                            color: Qt.rgba(1, 1, 1, 0.14)
                        }
                        Rectangle {
                            visible: modelData[1] !== "sep"
                            anchors.fill: parent; radius: 6
                            color: mh.hovered ? Theme.accent : "transparent"
                            Text {
                                x: 10; anchors.verticalCenter: parent.verticalCenter
                                text: modelData[0]
                                color: mh.hovered ? "white" : Theme.ink
                                font.family: Theme.font; font.pixelSize: 13
                            }
                            HoverHandler { id: mh }
                            TapHandler {
                                onTapped: {
                                    ctx.visible = false
                                    const a = modelData[1]
                                    if (a === "open") { if (it.item.entry) it.item.entry.execute(); else Quickshell.execDetached([it.item.id]) }
                                    else if (a === "open_dl") Quickshell.execDetached(["solyra-files", Theme.home + "/Downloads"])
                                    else if (a === "open_trash") Quickshell.execDetached(["xdg-open", "trash:///"])
                                    else if (a === "show") { for (const w of it.wins) w.activate() }
                                    else if (a === "quit") { for (const w of it.wins) w.close() }
                                    else if (a === "stop") stopProc.running = true
                                    else if (a === "toggle_mag") { Theme.dockMagnify = !Theme.dockMagnify; Theme.save() }
                                    else if (a === "settings") { if (it.dock.shell) it.dock.shell.control() }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Stop Running in Background: TERM, then KILL after 5 s.
    Process {
        id: stopProc
        command: ["sh", "-c", "pkill -TERM -i -f \"$1\"; sleep 5; pkill -KILL -i -f \"$1\" || true", "sh", it.item.id || ""]
    }
}
