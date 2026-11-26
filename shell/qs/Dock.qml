import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Services.SystemTray
import Quickshell.Widgets

// The dock: pinned apps, running apps, a divider, Downloads and Trash.
// Gaussian magnification (max 1.75x, sigma 1.4 cells), launch bounce, a dot
// under running apps and a hollow dot for apps running in the background
// (a tray item but no window) with "Stop Running in Background".
PanelWindow {
    id: dockWin
    property var screenModel
    property var shell
    screen: screenModel
    anchors.bottom: true
    implicitWidth: card.width + 200
    implicitHeight: 150
    exclusiveZone: 76
    color: "transparent"
    WlrLayershell.namespace: "solyra-dock"
    mask: Region { item: hit }

    readonly property var pinned: ["solyra-files", "firefox", "solyra-term", "org.gnome.TextEditor", "mpv", "solyra-settings"]
    readonly property int cell: 48
    readonly property int gap: 8
    property real mx: -1

    function entryFor(id) { return DesktopEntries.byId(id) || DesktopEntries.heuristicLookup(id) }
    function windowsOf(appId) { return ToplevelManager.toplevels.values.filter(w => w.appId && (w.appId === appId || w.appId.toLowerCase().indexOf(appId.toLowerCase()) >= 0 || appId.toLowerCase().indexOf(w.appId.toLowerCase()) >= 0)) }

    // Items: pinned, then unpinned running apps, then background apps.
    readonly property var items: {
        const out = []
        const seen = {}
        for (const id of pinned) {
            const e = entryFor(id)
            if (!e) continue
            out.push({ kind: "app", id: id, entry: e, appId: id })
            seen[id] = true
        }
        for (const w of ToplevelManager.toplevels.values) {
            if (!w.appId || seen[w.appId]) continue
            let dup = false
            for (const k in seen) if (windowsOf(k).indexOf(w) >= 0) dup = true
            if (dup) continue
            seen[w.appId] = true
            out.push({ kind: "app", id: w.appId, entry: entryFor(w.appId), appId: w.appId })
        }
        for (const t of SystemTray.items.values) {
            const id = (t.id || "").toLowerCase()
            if (!id || seen[id] || windowsOf(id).length) continue
            seen[id] = true
            out.push({ kind: "bg", id: id, entry: entryFor(id), appId: id, tray: t })
        }
        out.push({ kind: "sep" })
        out.push({ kind: "downloads", id: "downloads" })
        out.push({ kind: "trash", id: "trash" })
        return out
    }

    Item {
        id: hit
        anchors.bottom: parent.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        width: card.width; height: dockWin.mx >= 0 ? 140 : 72
    }

    Glass {
        id: card
        r: 24
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 4
        anchors.horizontalCenter: parent.horizontalCenter
        height: 66
        width: row.width + 24
        Behavior on width { NumberAnimation { duration: Theme.ms(90) } }
        HoverHandler { onPointChanged: dockWin.mx = point.position.x; onHoveredChanged: if (!hovered) dockWin.mx = -1 }

        Row {
            id: row
            x: 12
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 9
            spacing: dockWin.gap
            Repeater {
                model: dockWin.items
                DockItem {
                    required property var modelData
                    required property int index
                    item: modelData
                    dock: dockWin
                    centre: 12 + index * (dockWin.cell + dockWin.gap) + dockWin.cell / 2
                }
            }
        }
    }
}
