import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower
import Quickshell.Widgets

// The menu bar, as macOS 26/27 draws it: fully transparent over the desktop
// and over windows alike (no strip, no divider), white text with a faint
// shadow. Left: the Solyra mark, the active app's name in bold and that app's
// own menus. Right: battery, Wi-Fi, Focus, Control Center and the date
// ("Fri 27 Nov 6:01 pm"); each opens its glass menu, its symbol sitting in a
// soft glass capsule while open. The Dynamic Island sits in the middle.
PanelWindow {
    id: bar
    property var screenModel
    property var shell
    screen: screenModel
    anchors { top: true; left: true; right: true }
    implicitHeight: 32
    exclusiveZone: 32
    color: "transparent"
    WlrLayershell.namespace: "solyra-bar"

    readonly property var active: ToplevelManager.activeToplevel
    readonly property string appId: active ? (active.appId || "") : ""
    readonly property string appName: {
        if (!active) return "Finder"
        const known = { "solyra-files": "Finder", "solyra-term": "Terminal", "solyra-settings": "System Settings", "mpv": "Music", "firefox": "Firefox" }
        if (known[appId]) return known[appId]
        const e = DesktopEntries.heuristicLookup(appId)
        return e ? e.name : (appId || "Solyra")
    }

    // Each app's own menus (the active one's show, as on macOS). Rows are
    // [label, action, shortcut]; actions are real (see shell.menuAction).
    readonly property var editMenu: ["Edit", [["Undo", "key:ctrl+z", "⌃Z"], ["Redo", "key:ctrl+shift+z", "⇧⌃Z"], ["-"], ["Cut", "key:ctrl+x", "⌃X"], ["Copy", "key:ctrl+c", "⌃C"], ["Paste", "key:ctrl+v", "⌃V"], ["Select All", "key:ctrl+a", "⌃A"]]]
    readonly property var windowMenu: ["Window", [["Minimise", "minimize", "⌘M"], ["Zoom", "maximize"], ["Float / Tile", "float"], ["Snap Layouts…", "snap", "⌘Z"], ["-"], ["Mission Control", "overview", "⌘↑"], ["Auto-arrange Windows", "arrange"]]]
    readonly property var helpMenu: ["Help", [["Keyboard Shortcuts", "help"], ["About Solyra", "about"]]]
    readonly property var appMenus: {
        if (appId === "solyra-term") return [
            ["Shell", [["New Tab", "key:ctrl+shift+t", "⇧⌃T"], ["New Window", "new"], ["-"], ["Split Right", "key:ctrl+shift+d", "⇧⌃D"], ["Split Down", "key:ctrl+shift+e", "⇧⌃E"], ["-"], ["Close Tab", "key:ctrl+shift+w", "⇧⌃W"]]],
            ["Edit", [["Copy", "key:ctrl+shift+c", "⇧⌃C"], ["Paste", "key:ctrl+shift+v", "⇧⌃V"], ["Find…", "key:ctrl+shift+f", "⇧⌃F"]]],
            ["View", [["Command Palette", "key:ctrl+k", "⌃K"], ["Bigger", "key:ctrl+equal", "⌃+"], ["Smaller", "key:ctrl+minus", "⌃−"], ["-"], ["Enter Full Screen", "fullscreen"]]],
            windowMenu, helpMenu]
        if (appId === "firefox") return [
            ["File", [["New Tab", "key:ctrl+t", "⌃T"], ["New Window", "key:ctrl+n", "⌃N"], ["Open Location…", "key:ctrl+l", "⌃L"], ["-"], ["Close Tab", "key:ctrl+w", "⌃W"]]],
            editMenu,
            ["View", [["Reload Page", "key:ctrl+r", "⌃R"], ["Zoom In", "key:ctrl+equal", "⌃+"], ["Zoom Out", "key:ctrl+minus", "⌃−"], ["-"], ["Enter Full Screen", "fullscreen"]]],
            ["History", [["Back", "key:alt+Left", "⌥←"], ["Forward", "key:alt+Right", "⌥→"], ["Show All History", "key:ctrl+h", "⌃H"]]],
            ["Bookmarks", [["Bookmark This Page", "key:ctrl+d", "⌃D"], ["Show All Bookmarks", "key:ctrl+shift+o", "⇧⌃O"]]],
            windowMenu, helpMenu]
        if (appId === "mpv") return [
            ["File", [["Close Window", "close"]]],
            ["Song", [["Play / Pause", "media:play-pause", "Space"], ["Next", "media:next"], ["Previous", "media:previous"]]],
            ["Controls", [["Play / Pause", "media:play-pause"], ["Next", "media:next"], ["Previous", "media:previous"], ["-"], ["Increase Volume", "volume:up"], ["Decrease Volume", "volume:down"]]],
            ["View", [["Enter Full Screen", "fullscreen"]]],
            windowMenu, helpMenu]
        // Finder (no window or the Files app) and every other app.
        return [
            ["File", [["New Finder Window", "go:home", "⌘E"], ["New Folder", "key:ctrl+shift+n", "⇧⌃N"], ["-"], ["Close Window", "close"]]],
            editMenu,
            ["View", [["as Icons", "key:ctrl+1", "⌃1"], ["as List", "key:ctrl+2", "⌃2"], ["as Columns", "key:ctrl+3", "⌃3"], ["-"], ["Enter Full Screen", "fullscreen"], ["Mission Control", "overview"]]],
            ["Go", [["Home", "go:home"], ["Documents", "go:Documents"], ["Downloads", "go:Downloads"], ["Pictures", "go:Pictures"], ["Computer", "go:/"]]],
            windowMenu, helpMenu]
    }

    // Open a status menu by name (qs ipc call shell menu wifi|focus|battery),
    // on the focused screen's bar.
    function openMenu(name) {
        const m = { wifi: wifiMenu, focus: focusMenu, battery: batteryMenu }[name]
        for (const x of [wifiMenu, focusMenu, batteryMenu]) if (x !== m) x.open = false
        if (m) { m.open = !m.open; if (m.open) m.opened() }
    }
    Connections {
        target: bar.shell
        function onMenuRequested(name) {
            const f = Hyprland.focusedMonitor
            if (!f || f.name === bar.screen.name) bar.openMenu(name)
        }
    }

    // The bar's text and symbols cast a faint shadow, legible on any wallpaper.
    component Shadowed: MultiEffect {
        shadowEnabled: true
        shadowColor: Qt.rgba(0, 0, 0, 0.35)
        shadowBlur: 0.4
        shadowVerticalOffset: 1
    }

    Row {
        id: left
        anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
        spacing: 2
        layer.enabled: true
        layer.effect: Shadowed {}
        MenuTitle { icon: "solyra"; items: [["About Solyra", "about"], ["System Settings…", "settings"], ["-"], ["Mission Control", "overview"], ["-"], ["Lock Screen", "lock", "⌘L"], ["Log Out…", "logout"]]; shell: bar.shell }
        MenuTitle { label: bar.appName; bold: true; items: [["About " + bar.appName, "about"], ["-"], ["Hide " + bar.appName, "hide"], ["Quit " + bar.appName, "close", "⌘Q"]]; shell: bar.shell }
        Repeater {
            model: bar.appMenus
            MenuTitle { required property var modelData; label: modelData[0]; items: modelData[1]; shell: bar.shell }
        }
    }

    Row {
        id: right
        anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
        spacing: 4
        layer.enabled: true
        layer.effect: Shadowed {}

        Repeater {
            model: SystemTray.items
            Item {
                required property SystemTrayItem modelData
                width: 28; height: 24
                IconImage { anchors.centerIn: parent; implicitSize: 16; source: modelData.icon }
                TapHandler { onTapped: modelData.activate() }
                TapHandler { acceptedButtons: Qt.RightButton; onTapped: if (modelData.hasMenu) modelData.display(bar, x, 32) }
            }
        }

        // Battery: the symbol (fill level, bolt while charging) and its menu.
        BatteryMenu { id: batteryMenu; visible: UPower.displayDevice.isLaptopBattery; shell: bar.shell }
        WifiMenu { id: wifiMenu; shell: bar.shell }
        FocusMenu { id: focusMenu; shell: bar.shell }
        // Control Center.
        Item {
            width: 30; height: 24
            Rectangle { anchors.fill: parent; radius: 12; color: Qt.rgba(1, 1, 1, ccHov.hovered ? 0.08 : 0) }
            Icon { anchors.centerIn: parent; name: "toggles"; size: 16 }
            HoverHandler { id: ccHov }
            TapHandler { onTapped: bar.shell.control() }
        }
        // The date and time ("Fri 27 Nov 6:01 pm"); opens the Notification Center.
        Item {
            width: clockText.implicitWidth + 16; height: 24
            Rectangle { anchors.fill: parent; radius: 12; color: Qt.rgba(1, 1, 1, bar.shell && bar.shell.centerOpen ? 0.20 : (clockHov.hovered ? 0.08 : 0)) }
            Text {
                id: clockText
                anchors.centerIn: parent
                color: "white"; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.Medium
                text: Qt.formatDateTime(clock.date, "ddd d MMM  h:mm ap")
                SystemClock { id: clock; precision: SystemClock.Minutes }
            }
            HoverHandler { id: clockHov }
            TapHandler { onTapped: bar.shell.notificationCenter() }
        }
    }
}
