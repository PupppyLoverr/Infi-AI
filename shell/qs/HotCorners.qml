import QtQuick
import Quickshell
import Quickshell.Wayland

// Hot corners (macOS): push the pointer into a bottom corner and hold it
// there a moment. Bottom-left: Mission Control. Bottom-right: Control
// Center. Off in Control Center ("Hot Corners").
Scope {
    id: hc
    property var screenModel
    property var shell
    readonly property int dwell: 280

    component Corner: PanelWindow {
        id: c
        property bool right: false
        signal fired()
        screen: hc.screenModel
        visible: Theme.hotCorners
        anchors { bottom: true; left: !right; right: right }
        implicitWidth: 3; implicitHeight: 3
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "solyra-hotcorner"
        HoverHandler { id: hov; onHoveredChanged: if (hovered) hold.restart(); else hold.stop() }
        Timer { id: hold; interval: hc.dwell; onTriggered: if (hov.hovered) c.fired() }
    }

    Corner { onFired: hc.shell.toggleOverview() }
    Corner { right: true; onFired: hc.shell.control() }
}
