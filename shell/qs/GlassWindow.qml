import QtQuick
import Quickshell
import Quickshell.Wayland

// One piece of Liquid Glass as its own layer surface, exactly the size of the
// card. Hyprland's acrylic lens bends the backdrop along the edges of each
// surface's box, so a card has to be its own surface for its edges to
// refract the way Apple's glass does (a panel holding many cards would only
// bend at the panel's edges). Position it from the top-left (gx, gy) or, with
// fromRight, from the top-right; the content goes inside the glass.
PanelWindow {
    id: w
    property real gx: 0
    property real gy: 0
    property bool fromRight: false
    property real r: 22
    property string kind: "clear"
    property alias glass: g
    default property alias content: g.content
    anchors { top: true; left: !fromRight; right: fromRight }
    margins { top: gy; left: fromRight ? 0 : gx; right: fromRight ? gx : 0 }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-glass"
    Glass { id: g; anchors.fill: parent; r: w.r; kind: w.kind; shadow: false }
}
