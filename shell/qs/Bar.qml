import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower
import Quickshell.Widgets

// macOS 27 Golden Gate Menu Bar:
// Fully see-through floating bar over the desktop with pill hover highlights,
// fading into a crisp Liquid Glass strip + hairline divider when windows open.
// Left: Solyra emblem, bold active app, File / Edit / View / Go / Window / Help
// with shortcut hints. Right: tray, battery, Wi-Fi, Spotlight, authentic
// Control Center toggle glyph, and clock (opens Notification Center).
PanelWindow {
    id: bar
    property var screenModel
    property var shell
    screen: screenModel
    anchors { top: true; left: true; right: true }
    implicitHeight: 30
    exclusiveZone: 30
    color: "transparent"
    WlrLayershell.namespace: "solyra-bar"

    readonly property var active: ToplevelManager.activeToplevel
    readonly property string appName: {
        if (!active) return "Finder"
        const e = DesktopEntries.heuristicLookup(active.appId)
        return e ? e.name : (active.appId || "Solyra")
    }

    readonly property var workspace: {
        const m = Hyprland.monitorFor(bar.screen)
        return m ? m.activeWorkspace : Hyprland.focusedWorkspace
    }
    readonly property bool windowsOpen: workspace ? workspace.toplevels.values.length > 0 : ToplevelManager.toplevels.values.length > 0
    readonly property bool solid: windowsOpen || Theme.reduceTransparency

    // Subtle readability gradient when floating over bright galaxy/sky areas
    Rectangle {
        anchors.fill: parent
        visible: !bar.solid
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(0, 0, 0, 0.24) }
            GradientStop { position: 1.0; color: "transparent" }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.glassFill
        opacity: bar.solid ? (Theme.reduceTransparency ? 1 : 0.56) : 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220); easing.type: Easing.OutCubic } }
    }
    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 1
        color: Theme.dark ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(0, 0, 0, 0.10)
        opacity: bar.solid ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
    }

    Row {
        anchors { left: parent.left; leftMargin: 8; verticalCenter: parent.verticalCenter }
        spacing: 2
        MenuTitle {
            label: "◉"; bold: true; shell: bar.shell
            items: [
                ["About Solyra", "about", ""],
                ["System Settings…", "settings", "⌘,"],
                ["-"],
                ["Lock Screen", "lock", "⌃⌘Q"],
                ["Log Out…", "logout", "⇧⌘Q"]
            ]
        }
        MenuTitle {
            label: bar.appName; bold: true; shell: bar.shell
            items: [
                ["Hide " + bar.appName, "hide", "⌘H"],
                ["Quit " + bar.appName, "close", "⌘Q"]
            ]
        }
        MenuTitle {
            label: "File"; shell: bar.shell
            items: [
                ["New Window", "new", "⌘N"],
                ["Close Window", "close", "⌘W"]
            ]
        }
        MenuTitle {
            label: "Edit"; shell: bar.shell
            items: [
                ["Undo", "key:ctrl+z", "⌘Z"],
                ["-"],
                ["Cut", "key:ctrl+x", "⌘X"],
                ["Copy", "key:ctrl+c", "⌘C"],
                ["Paste", "key:ctrl+v", "⌘V"],
                ["Select All", "key:ctrl+a", "⌘A"]
            ]
        }
        MenuTitle {
            label: "View"; shell: bar.shell
            items: [
                ["Enter Full Screen", "fullscreen", "⌃⌘F"],
                ["Mission Control", "overview", "⌃↑"]
            ]
        }
        MenuTitle {
            label: "Go"; shell: bar.shell
            items: [
                ["Home", "go:home", "⇧⌘H"],
                ["Documents", "go:docs", "⇧⌘O"],
                ["Downloads", "go:downloads", "⌥⌘L"],
                ["Applications", "spotlight", "⌘Space"]
            ]
        }
        MenuTitle {
            label: "Window"; shell: bar.shell
            items: [
                ["Minimise", "minimize", "⌘M"],
                ["Zoom", "maximize", ""],
                ["Float / Tile", "float", ""],
                ["Snap Layouts…", "snap", "⌘Z"],
                ["-"],
                ["Auto-arrange Windows", "arrange", ""]
            ]
        }
        MenuTitle {
            label: "Help"; shell: bar.shell
            items: [
                ["Keyboard Shortcuts", "help", "⌘?"]
            ]
        }
    }

    Row {
        anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
        spacing: 4
        Repeater {
            model: SystemTray.items
            Rectangle {
                required property SystemTrayItem modelData
                width: 24; height: 22; radius: 6
                anchors.verticalCenter: parent.verticalCenter
                color: trayH.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
                IconImage {
                    anchors.centerIn: parent
                    implicitSize: 15
                    source: modelData.icon
                }
                HoverHandler { id: trayH }
                TapHandler { onTapped: modelData.activate() }
                TapHandler { acceptedButtons: Qt.RightButton; onTapped: if (modelData.hasMenu) modelData.display(bar, x, 30) }
            }
        }
        Row {
            visible: UPower.displayDevice.isLaptopBattery
            spacing: 4
            anchors.verticalCenter: parent.verticalCenter
            leftPadding: 4; rightPadding: 4
            Text { text: Math.round(UPower.displayDevice.percentage * (UPower.displayDevice.percentage > 1 ? 1 : 100)) + "%"; color: Theme.ink; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Medium }
            Icon { name: "battery"; size: 17; color: Theme.ink }
        }
        Rectangle {
            width: 26; height: 22; radius: 6
            anchors.verticalCenter: parent.verticalCenter
            color: wifiH.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
            Icon { anchors.centerIn: parent; name: "wifi"; size: 15; color: Theme.ink }
            HoverHandler { id: wifiH }
            TapHandler { onTapped: if (bar.shell) bar.shell.control() }
        }
        Rectangle {
            width: 26; height: 22; radius: 6
            anchors.verticalCenter: parent.verticalCenter
            color: searchH.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
            Icon { anchors.centerIn: parent; name: "search"; size: 15; color: Theme.ink }
            HoverHandler { id: searchH }
            TapHandler { onTapped: if (bar.shell) bar.shell.spotlight() }
        }
        Rectangle {
            width: 28; height: 22; radius: 6
            anchors.verticalCenter: parent.verticalCenter
            color: ccH.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
            Icon { anchors.centerIn: parent; name: "controlcenter"; size: 16; color: Theme.ink }
            HoverHandler { id: ccH }
            TapHandler { onTapped: if (bar.shell) bar.shell.control() }
        }
        Rectangle {
            height: 22; width: clkTxt.implicitWidth + 14; radius: 6
            anchors.verticalCenter: parent.verticalCenter
            color: clkH.hovered ? Qt.rgba(1, 1, 1, 0.14) : "transparent"
            Text {
                id: clkTxt
                anchors.centerIn: parent
                color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
                text: Qt.formatDateTime(clock.date, "ddd MMM d  HH:mm")
                SystemClock { id: clock; precision: SystemClock.Minutes }
            }
            HoverHandler { id: clkH }
            TapHandler { onTapped: if (bar.shell) bar.shell.notificationCenter() }
        }
    }
}
