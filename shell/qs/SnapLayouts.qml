import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland

// Windows 11-style snap layouts (Super+Z, the Window menu, the green
// button's hover in our apps): pick a layout, then the zone for the focused
// window. Zones are placed exactly with Hyprland dispatchers.
PanelWindow {
    id: s
    property var shell
    property bool open: false
    visible: open
    anchors { top: true }
    margins.top: 40
    implicitWidth: 460; implicitHeight: 196
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-snap"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    // Layouts as zone lists in fractions of the usable area: [x, y, w, h].
    readonly property var layouts: [
        [[0, 0, .5, 1], [.5, 0, .5, 1]],
        [[0, 0, .667, 1], [.667, 0, .333, 1]],
        [[0, 0, .333, 1], [.333, 0, .334, 1], [.667, 0, .333, 1]],
        [[0, 0, .5, .5], [.5, 0, .5, .5], [0, .5, .5, .5], [.5, .5, .5, .5]],
        [[0, 0, .5, 1], [.5, 0, .5, .5], [.5, .5, .5, .5]],
        [[.125, .05, .75, .9]]
    ]

    function place(z) {
        const scr = s.screen
        const top = 30 + 8, bottom = 76 + 8, gap = 8
        const W = scr.width - 2 * gap, H = scr.height - top - bottom
        const x = Math.round(gap + z[0] * W + (z[0] > 0 ? gap / 2 : 0))
        const y = Math.round(top + z[1] * H + (z[1] > 0 ? gap / 2 : 0))
        const w = Math.round(z[2] * W - (z[0] > 0 ? gap / 2 : 0) - (z[0] + z[2] < 0.999 ? gap / 2 : 0))
        const h = Math.round(z[3] * H - (z[1] > 0 ? gap / 2 : 0) - (z[1] + z[3] < 0.999 ? gap / 2 : 0))
        Hyprland.dispatch("hl.dsp.window.float({ action = 'enable' })")
        Hyprland.dispatch("hl.dsp.window.resize({ x = " + w + ", y = " + h + " })")
        Hyprland.dispatch("hl.dsp.window.move({ x = " + x + ", y = " + y + " })")
        open = false
    }

    Glass {
        anchors.fill: parent; r: 20
        Text { x: 16; y: 12; text: "Snap Layouts"; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
        Grid {
            x: 16; y: 36; columns: 3; spacing: 12
            Repeater {
                model: s.layouts
                Rectangle {
                    required property var modelData
                    width: 132; height: 70; radius: 10; color: Theme.fill
                    Repeater {
                        model: modelData
                        Rectangle {
                            required property var modelData
                            x: 4 + modelData[0] * 124; y: 4 + modelData[1] * 62
                            width: modelData[2] * 124 - 3; height: modelData[3] * 62 - 3; radius: 6
                            color: zh.hovered ? Theme.accent : Qt.rgba(1, 1, 1, 0.22)
                            HoverHandler { id: zh }
                            TapHandler { onTapped: s.place(modelData) }
                        }
                    }
                }
            }
        }
    }
}
