import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Widgets

Item {
    id: it
    property var item
    property var dock
    property real centre
    readonly property bool sep: item.kind === "sep"
    readonly property var wins: item.kind === "app" ? dock.windowsOf(item.appId) : []
    readonly property real scaleF: {
        if (sep || dock.mx < 0 || Theme.reduceMotion) return 1
        const d = (dock.mx - centre) / (dock.cell + dock.gap)
        return 1 + 0.75 * Math.exp(-(d * d) / (2 * 1.4 * 1.4))
    }
    width: sep ? 9 : dock.cell * scaleF
    height: sep ? 48 : dock.cell * scaleF
    Behavior on width { NumberAnimation { duration: 80 } }
    Behavior on height { NumberAnimation { duration: 80 } }

    Rectangle { visible: it.sep; anchors.centerIn: parent; width: 1; height: 38; color: Qt.rgba(1, 1, 1, 0.3) }

    Item {
        id: iconBox
        visible: !it.sep
        width: parent.width; height: parent.height
        y: bounce.offset
        // Layered icon: our bundled app art when we have one, else the theme icon.
        Image {
            id: art
            anchors.fill: parent
            sourceSize: Qt.size(128, 128)
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
    }
    QtObject { id: bounce; property real offset: 0 }
    SequentialAnimation {
        id: bounceAnim
        NumberAnimation { target: bounce; property: "offset"; to: -6; duration: Theme.ms(180); easing.type: Easing.OutQuad }
        NumberAnimation { target: bounce; property: "offset"; to: 0; duration: Theme.ms(220); easing.type: Easing.OutBounce }
    }

    // Running: solid dot. Background (tray only): hollow dot.
    Rectangle {
        visible: it.wins.length > 0 || it.item.kind === "bg"
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.bottom
        anchors.topMargin: 3
        width: 5; height: 5; radius: 2.5
        color: it.item.kind === "bg" ? "transparent" : Qt.rgba(1, 1, 1, 0.9)
        border.width: it.item.kind === "bg" ? 1.2 : 0
        border.color: Qt.rgba(1, 1, 1, 0.9)
    }

    // Glass tooltip: its own popup surface above the dock.
    PopupWindow {
        visible: hover.hovered && !it.sep
        anchor.item: it
        anchor.rect.x: (it.width - implicitWidth) / 2
        anchor.rect.y: -implicitHeight - 14
        implicitWidth: tip.implicitWidth + 24; implicitHeight: 28
        color: "transparent"
        Glass { anchors.fill: parent; r: 14; kind: "clear"; shadow: false }
        Text {
            id: tip; anchors.centerIn: parent; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            text: it.item.kind === "downloads" ? "Downloads" : it.item.kind === "trash" ? "Trash" : it.item.kind === "min" ? (it.item.title || "Window") : (it.item.entry ? it.item.entry.name : (it.dock.bundled[it.item.id] || it.item.id)) + (it.item.kind === "bg" ? " (background)" : "")
        }
    }

    HoverHandler { id: hover }
    TapHandler {
        enabled: !it.sep
        onTapped: {
            const k = it.item.kind
            if (k === "downloads") Quickshell.execDetached(["xdg-open", Theme.home + "/Downloads"])
            else if (k === "trash") Quickshell.execDetached(["xdg-open", "trash:///"])
            else if (k === "bg") { if (it.item.tray) it.item.tray.activate() }
            else if (k === "min") it.dock.restore(it.item.address)
            else if (it.wins.length) it.wins[0].activate()
            else if (it.item.entry) { bounceAnim.restart(); it.item.entry.execute() }
            else { bounceAnim.restart(); Quickshell.execDetached([it.item.id]) }
        }
    }
    TapHandler {
        enabled: !it.sep && it.item.kind !== "downloads" && it.item.kind !== "trash" && it.item.kind !== "min"
        acceptedButtons: Qt.RightButton
        onTapped: ctx.visible = true
    }
    PopupWindow {
        id: ctx
        anchor.item: it
        anchor.rect.y: -150
        implicitWidth: 240; implicitHeight: menuCol.height + 12
        color: "transparent"
        Glass {
            anchors.fill: parent; r: 12
            Column {
                id: menuCol
                x: 6; y: 6; width: parent.width - 12
                Repeater {
                    model: it.item.kind === "bg" ? [["Open", "open"], ["Stop Running in Background", "stop"]]
                         : it.wins.length ? [["Show All Windows", "show"], ["Quit", "quit"]] : [["Open", "open"]]
                    Rectangle {
                        required property var modelData
                        width: menuCol.width; height: 26; radius: 6
                        color: mh.hovered ? Theme.accent : "transparent"
                        Text { x: 10; anchors.verticalCenter: parent.verticalCenter; text: modelData[0]; color: mh.hovered ? "white" : Theme.ink; font.family: Theme.font; font.pixelSize: 13 }
                        HoverHandler { id: mh }
                        TapHandler {
                            onTapped: {
                                ctx.visible = false
                                const a = modelData[1]
                                if (a === "open") { if (it.item.entry) it.item.entry.execute() }
                                else if (a === "show") { for (const w of it.wins) w.activate() }
                                else if (a === "quit") { for (const w of it.wins) w.close() }
                                else if (a === "stop") stopProc.running = true
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
