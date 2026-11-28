import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland

// Cupertino Application Switcher (Alt/Super+Tab): centered frosted glass pill
// with 64px icons, rounded selection highlight, and bold active app name.
Scope {
    id: sw
    required property var shell
    property bool open: false
    property int selectedIndex: 0

    readonly property var apps: {
        const seen = {}
        const out = []
        const list = Hyprland.toplevels ? Hyprland.toplevels.values : []
        for (let i = 0; i < list.length; i++) {
            const t = list[i]
            if (!t) continue
            const cls = (t.lastIpcObject && t.lastIpcObject.class) ? t.lastIpcObject.class : (t.title || "App")
            const key = cls.toLowerCase()
            if (seen[key]) continue
            seen[key] = true
            out.push({
                label: key.indexOf("files") >= 0 ? "Finder"
                    : key.indexOf("term") >= 0 ? "Terminal"
                    : key.indexOf("firefox") >= 0 ? "Safari"
                    : key.indexOf("settings") >= 0 ? "Settings"
                    : cls,
                icon: key.indexOf("files") >= 0 ? "folder"
                    : key.indexOf("term") >= 0 ? "terminal"
                    : key.indexOf("settings") >= 0 ? "settings"
                    : "desktop",
                address: t.address || ""
            })
        }
        if (out.length === 0) {
            out.push({ label: "Finder", icon: "folder", address: "" })
            out.push({ label: "Terminal", icon: "terminal", address: "" })
            out.push({ label: "Settings", icon: "settings", address: "" })
        }
        return out
    }

    function show(on) {
        open = !!on
        if (open) selectedIndex = apps.length > 1 ? 1 : 0
    }

    function activate(idx) {
        const i = Math.max(0, Math.min(apps.length - 1, idx))
        const item = apps[i]
        open = false
        if (item && item.address) Quickshell.execDetached(["hyprctl", "dispatch", "focuswindow", "address:0x" + item.address])
    }

    PanelWindow {
        visible: sw.open
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-switcher"
        WlrLayershell.keyboardFocus: sw.open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        MouseArea { anchors.fill: parent; onClicked: sw.open = false }

        Item {
            anchors.centerIn: parent
            width: Math.max(240, row.implicitWidth + 36)
            height: 132
            focus: sw.open
            Keys.onEscapePressed: sw.open = false
            Keys.onLeftPressed: sw.selectedIndex = (sw.selectedIndex - 1 + sw.apps.length) % sw.apps.length
            Keys.onRightPressed: sw.selectedIndex = (sw.selectedIndex + 1) % sw.apps.length
            Keys.onTabPressed: sw.selectedIndex = (sw.selectedIndex + 1) % sw.apps.length
            Keys.onReturnPressed: sw.activate(sw.selectedIndex)

            Glass { anchors.fill: parent; kind: "dock"; r: 24 }

            Row {
                id: row
                anchors.centerIn: parent
                spacing: 12
                Repeater {
                    model: sw.apps
                    Item {
                        required property var modelData
                        required property int index
                        width: 88; height: 98
                        Rectangle {
                            anchors.fill: parent
                            radius: 16
                            color: sw.selectedIndex === index ? Qt.rgba(1, 1, 1, 0.18) : "transparent"
                            border.width: sw.selectedIndex === index ? 1 : 0
                            border.color: Qt.rgba(1, 1, 1, 0.28)
                        }
                        Rectangle {
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: 12; width: 56; height: 56; radius: 14
                            color: Qt.rgba(1, 1, 1, 0.12)
                            Icon { anchors.centerIn: parent; name: modelData.icon; size: 30 }
                        }
                        Text {
                            anchors.horizontalCenter: parent.horizontalCenter
                            y: 74; width: 82
                            horizontalAlignment: Text.AlignHCenter
                            elide: Text.ElideRight
                            text: modelData.label
                            color: "white"
                            font.family: Theme.font; font.pixelSize: 12
                            font.weight: sw.selectedIndex === index ? Theme.wTitle : Theme.wBody
                        }
                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onEntered: sw.selectedIndex = index
                            onClicked: sw.activate(index)
                        }
                    }
                }
            }
        }
    }
}
