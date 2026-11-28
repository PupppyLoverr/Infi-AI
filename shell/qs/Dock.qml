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
    readonly property bool isUnity: Theme.layoutStyle === "unity"
    readonly property bool isTaskbar: Theme.layoutStyle === "taskbar"
    readonly property bool isMinimal: Theme.layoutStyle === "minimal"
    anchors.bottom: !isUnity
    anchors.left: isUnity || isTaskbar
    anchors.right: isTaskbar
    anchors.top: isUnity
    // Resting: surface is exactly the glass card so Hyprland's acrylic lens
    // bends the backdrop right along the dock's own edges. Expanding on hover
    // gives room for wave magnification and tooltips when enabled.
    readonly property bool activeHover: !isUnity && mx >= 0 && Theme.dockMagnify && !Theme.reduceMotion
    readonly property bool hidden: (Theme.dockAutohide || isMinimal) && pointerMx < 0 && forcedMx < 0
    implicitWidth: isUnity ? (hidden ? 8 : card.width + 6) : (isTaskbar ? (screenModel ? screenModel.width : 1280) : (activeHover ? card.width + 148 : card.width))
    implicitHeight: isUnity ? (screenModel ? screenModel.height : 800) : (activeHover ? 140 : (hidden ? 8 : (isTaskbar ? 52 : card.height + 4)))
    exclusiveZone: (Theme.dockAutohide || isMinimal) ? 0 : (isUnity ? card.width + 6 : (isTaskbar ? 52 : card.height + 4))
    color: "transparent"
    WlrLayershell.namespace: "solyra-dock"

    readonly property var pinned: ["solyra-files", "firefox", "solyra-term", "org.gnome.TextEditor", "mpv", "solyra-settings"]
    readonly property var bundled: ({ "solyra-files": "Finder", "solyra-term": "Terminal", "solyra-settings": "Settings" })
    property alias dockCard: card
    property alias dockRow: row
    readonly property int cell: 46
    readonly property int gap: 6
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
    property real pointerMx: -1
    property real forcedMx: -1
    readonly property real mx: forcedMx >= 0 ? forcedMx : pointerMx
    signal downloadsStackToggled()

    // Bring a minimised window back to the focused desktop and focus it.
    function restore(address) {
        const ws = Hyprland.focusedWorkspace ? Hyprland.focusedWorkspace.id : 1
        Hyprland.dispatch("hl.dsp.window.move({ workspace = " + ws + ", window = 'address:0x" + address + "' })")
        Hyprland.dispatch("hl.dsp.focus({ window = 'address:0x" + address + "' })")
    }
    // An item's resting centre along the dock (dividers are narrower than
    // icons), for the magnification curve.
    function centreOf(i) {
        let x = 8
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


    Glass {
        id: card
        kind: "dock"
        r: dockWin.isTaskbar ? 0 : 20
        shadow: false
        anchors.bottom: dockWin.isUnity ? undefined : parent.bottom
        anchors.bottomMargin: dockWin.isUnity ? 0 : (dockWin.isTaskbar ? 0 : (dockWin.hidden ? -(height + 12) : 4))
        anchors.left: dockWin.isUnity || dockWin.isTaskbar ? parent.left : undefined
        anchors.right: dockWin.isTaskbar ? parent.right : undefined
        anchors.leftMargin: dockWin.isUnity ? (dockWin.hidden ? -(width + 12) : 6) : 0
        anchors.verticalCenter: dockWin.isUnity ? parent.verticalCenter : undefined
        anchors.horizontalCenter: (dockWin.isUnity || dockWin.isTaskbar) ? undefined : parent.horizontalCenter
        Behavior on anchors.bottomMargin { enabled: !Theme.reduceMotion; NumberAnimation { duration: Theme.ms(220); easing.type: Easing.OutCubic } }
        height: dockWin.isUnity ? Math.min(parent.height - 80, row.height + 16) : (dockWin.isTaskbar ? 52 : 62)
        width: dockWin.isUnity ? 62 : (dockWin.isTaskbar ? parent.width : row.width + 16)
        Behavior on width {
            enabled: !Theme.reduceMotion && !dockWin.isTaskbar && !dockWin.isUnity
            SpringAnimation { spring: 4.8; damping: 0.48; epsilon: 0.3 }
        }
        HoverHandler {
            onPointChanged: dockWin.pointerMx = dockWin.isUnity ? point.position.y : point.position.x
            onHoveredChanged: if (!hovered) dockWin.pointerMx = -1
        }

        Grid {
            id: row
            columns: dockWin.isUnity ? 1 : 64
            x: dockWin.isUnity ? 8 : (dockWin.isTaskbar ? (parent.width - width) / 2 : 8)
            anchors.bottom: dockWin.isUnity ? undefined : parent.bottom
            anchors.bottomMargin: dockWin.isUnity ? 0 : (dockWin.isTaskbar ? 3 : 6)
            anchors.verticalCenter: dockWin.isUnity ? parent.verticalCenter : undefined
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
        Row {
            visible: dockWin.isTaskbar
            anchors.right: parent.right; anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            spacing: 12
            Icon { name: "search"; size: 15; opacity: 0.9; TapHandler { onTapped: if (dockWin.shell) dockWin.shell.toggleSearch() } }
            Icon { name: "sliders"; size: 15; opacity: 0.9; TapHandler { onTapped: if (dockWin.shell) dockWin.shell.toggleControlCenter() } }
            Text {
                text: Qt.formatDateTime(new Date(), "h:mm AP")
                color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Theme.wTitle
            }
        }
    }
}
