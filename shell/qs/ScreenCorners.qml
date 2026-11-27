import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Wayland

// Rounded screen corners, as a MacBook's display has: four black corner
// arcs over everything, click-through (an empty input region).
PanelWindow {
    id: c
    property var screenModel
    screen: screenModel
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-corners"
    mask: Region {}
    readonly property real r: 12

    component Corner: Shape {
        property int rot: 0
        width: c.r; height: c.r
        rotation: rot
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            fillColor: "black"; strokeWidth: 0
            startX: 0; startY: 0
            PathLine { x: c.r; y: 0 }
            PathArc { x: 0; y: c.r; radiusX: c.r; radiusY: c.r; direction: PathArc.Counterclockwise }
            PathLine { x: 0; y: 0 }
        }
    }
    Corner { x: 0; y: 0; rot: 0 }
    Corner { x: c.width - c.r; y: 0; rot: 90 }
    Corner { x: c.width - c.r; y: c.height - c.r; rot: 180 }
    Corner { x: 0; y: c.height - c.r; rot: 270 }
}
