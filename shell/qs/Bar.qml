import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import Quickshell.Hyprland
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower
import Quickshell.Widgets

// The menu bar: logo menu, the active app's name and menus, status items,
// Control Center and the clock ("Thu Nov 26  11:40"). As in macOS 27 it is
// fully see-through over the desktop (no fill, no divider); once a window is
// open on this screen's workspace a glass strip and a hairline divider fade in
// so the menus stay readable over it.
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

    Rectangle {
        anchors.fill: parent
        color: Theme.glassFill
        opacity: bar.solid ? (Theme.reduceTransparency ? 1 : 0.55) : 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220); easing.type: Easing.OutCubic } }
    }
    // The divider: only over windows.
    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: 1
        color: Theme.dark ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(0, 0, 0, 0.10)
        opacity: bar.solid ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.ms(220) } }
    }

    Row {
        anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
        spacing: 20
        MenuTitle { label: "◉"; bold: true; items: [["About Solyra", "about"], ["System Settings…", "settings"], ["-"], ["Lock Screen", "lock"], ["Log Out…", "logout"]]; shell: bar.shell }
        MenuTitle { label: bar.appName; bold: true; items: [["Hide " + bar.appName, "hide"], ["Quit " + bar.appName, "close"]]; shell: bar.shell }
        MenuTitle { label: "File"; items: [["New Window", "new"], ["Close Window", "close"]]; shell: bar.shell }
        MenuTitle { label: "Edit"; items: [["Undo", "key:ctrl+z"], ["Cut", "key:ctrl+x"], ["Copy", "key:ctrl+c"], ["Paste", "key:ctrl+v"], ["Select All", "key:ctrl+a"]]; shell: bar.shell }
        MenuTitle { label: "View"; items: [["Enter Full Screen", "fullscreen"], ["Mission Control", "overview"]]; shell: bar.shell }
        MenuTitle { label: "Window"; items: [["Minimise", "minimize"], ["Zoom", "maximize"], ["Float / Tile", "float"], ["Snap Layouts…", "snap"], ["-"], ["Auto-arrange Windows", "arrange"]]; shell: bar.shell }
        MenuTitle { label: "Help"; items: [["Keyboard Shortcuts", "help"]]; shell: bar.shell }
    }

    Row {
        anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
        spacing: 14
        Repeater {
            model: SystemTray.items
            IconImage {
                required property SystemTrayItem modelData
                anchors.verticalCenter: parent.verticalCenter
                implicitSize: 16
                source: modelData.icon
                TapHandler { onTapped: modelData.activate() }
                TapHandler { acceptedButtons: Qt.RightButton; onTapped: if (modelData.hasMenu) modelData.display(bar, x, 30) }
            }
        }
        Row {
            visible: UPower.displayDevice.isLaptopBattery
            spacing: 4
            anchors.verticalCenter: parent.verticalCenter
            Text { text: Math.round(UPower.displayDevice.percentage * (UPower.displayDevice.percentage > 1 ? 1 : 100)) + "%"; color: Theme.ink; font.family: Theme.font; font.pixelSize: 12 }
            Icon { name: "battery"; size: 18; color: Theme.ink }
        }
        Icon { anchors.verticalCenter: parent.verticalCenter; name: "wifi"; size: 16; color: Theme.ink }
        Icon {
            anchors.verticalCenter: parent.verticalCenter; name: "search"; size: 16; color: Theme.ink
            TapHandler { onTapped: bar.shell.spotlight() }
        }
        Icon {
            anchors.verticalCenter: parent.verticalCenter; name: "settings"; size: 16; color: Theme.ink
            TapHandler { onTapped: bar.shell.control() }
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            text: Qt.formatDateTime(clock.date, "ddd MMM d  HH:mm")
            SystemClock { id: clock; precision: SystemClock.Minutes }
        }
    }
}
