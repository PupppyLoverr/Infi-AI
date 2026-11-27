import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Services.SystemTray
import Quickshell.Widgets

// The dock: pinned apps, running apps, a divider, Downloads and Trash.
// Gaussian magnification (max 1.75x, sigma 1.4 cells), launch bounce, a dot
// under running apps and a hollow dot for apps running in the background
// (a tray item but no window) with "Stop Running in Background". Minimised
// windows (Window ▸ Minimise) sit on the right before Downloads and Trash,
// as on macOS; a click brings one back to the current desktop.
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
    readonly property var bundled: ({ "solyra-files": "Finder", "solyra-term": "Terminal", "solyra-settings": "Settings" })
    readonly property int cell: 48
    readonly property int gap: 2
    // Recently used apps that are not pinned (macOS's recent-apps section):
    // remembered as they come to the front, newest first, three kept.
    property var recent: []
    Connections {
        target: ToplevelManager
        function onActiveToplevelChanged() {
            const w = ToplevelManager.activeToplevel
            if (!w || !w.appId || dockWin.pinned.indexOf(w.appId) >= 0) return
            dockWin.recent = [w.appId].concat(dockWin.recent.filter(x => x !== w.appId)).slice(0, 3)
        }
    }
    property real mx: -1

    // Bring a minimised window back to the focused desktop and focus it.
    function restore(address) {
        const ws = Hyprland.focusedWorkspace ? Hyprland.focusedWorkspace.id : 1
        Hyprland.dispatch("hl.dsp.window.move({ workspace = " + ws + ", window = 'address:0x" + address + "' })")
        Hyprland.dispatch("hl.dsp.focus({ window = 'address:0x" + address + "' })")
    }
    // An item's resting centre along the dock (dividers are narrower than
    // icons), for the magnification curve.
    function centreOf(i) {
        let x = 12
        for (let k = 0; k < i; k++) x += (items[k].kind === "sep" ? 9 : cell) + gap
        return x + (items[i] && items[i].kind === "sep" ? 4.5 : cell / 2)
    }
    function entryFor(id) { return DesktopEntries.byId(id) || DesktopEntries.heuristicLookup(id) }
    function windowsOf(appId) { return ToplevelManager.toplevels.values.filter(w => w.appId && (w.appId === appId || w.appId.toLowerCase().indexOf(appId.toLowerCase()) >= 0 || appId.toLowerCase().indexOf(w.appId.toLowerCase()) >= 0)) }

    // Items, in macOS 26/27's three sections: pinned apps | recent and running
    // apps (and apps running in the background) | minimised windows,
    // Downloads and Trash.
    readonly property var items: {
        const out = []
        const seen = {}
        // Desktop entries load asynchronously: touch the list so this
        // binding re-runs once they arrive.
        const _apps = DesktopEntries.applications.values.length
        for (const id of pinned) {
            const e = entryFor(id)
            // Keep our own apps even before (or without) a desktop entry.
            if (!e && !(id in bundled)) continue
            out.push({ kind: "app", id: id, entry: e, appId: id })
            seen[id] = true
        }
        const middle = []
        for (const w of ToplevelManager.toplevels.values) {
            if (!w.appId || seen[w.appId]) continue
            let dup = false
            for (const k in seen) if (windowsOf(k).indexOf(w) >= 0) dup = true
            if (dup) continue
            seen[w.appId] = true
            middle.push({ kind: "app", id: w.appId, entry: entryFor(w.appId), appId: w.appId })
        }
        for (const id of recent) {
            if (seen[id]) continue
            const e = entryFor(id)
            if (!e) continue
            seen[id] = true
            middle.push({ kind: "app", id: id, entry: e, appId: id })
        }
        for (const t of SystemTray.items.values) {
            const id = (t.id || "").toLowerCase()
            if (!id || seen[id] || windowsOf(id).length) continue
            seen[id] = true
            middle.push({ kind: "bg", id: id, entry: entryFor(id), appId: id, tray: t })
        }
        if (middle.length) { out.push({ kind: "sep" }); for (const x of middle) out.push(x) }
        out.push({ kind: "sep" })
        for (const t of Hyprland.toplevels.values) {
            if (!t.workspace || t.workspace.name !== "special:minimized") continue
            const appId = t.wayland ? t.wayland.appId : ""
            out.push({ kind: "min", id: "min-" + t.address, appId: appId, entry: entryFor(appId), title: t.title, address: t.address })
        }
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
        kind: "clear"
        r: 24
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 4
        anchors.horizontalCenter: parent.horizontalCenter
        height: 64
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
                    centre: dockWin.centreOf(index)
                }
            }
        }
    }
}
