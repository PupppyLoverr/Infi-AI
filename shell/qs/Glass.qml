import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// Liquid Glass, tuned to macOS 27 Golden Gate & Cupertino's optical model.
// Hyprland's acrylic lens bends the backdrop at each surface's edge
// (ignore_alpha 0.16, so only the card's own shape is glass). This draws the
// optical material on top in three kinds:
//  - "clear"   Control Center tiles, the dock, menus: vivid refracted backdrop;
//  - "frosted" Spotlight's results, notifications: softer colour-washed body;
//  - "smoke"   Spotlight's search capsule: dark, smoky glass.
// Dual-layer specular rim: crisp 1px outer bevel (bright top + bounce bottom,
// quiet sides) plus a delicate inner counter-rim and top meniscus sheen.
// Bodies stay strictly above ignore_alpha (0.16) so they are always blurred;
// shadows stay below it so they never turn into blurred boxes.
Item {
    id: g
    property real r: Theme.radius
    property bool dark: Theme.dark
    property bool shadow: true
    property string kind: "clear"
    default property alias content: holder.data

    readonly property real slider: Theme.glass / 100
    readonly property color body: {
        if (Theme.reduceTransparency) return dark ? "#f21c1c1f" : "#f6f6f6f8"
        if (kind === "smoke") return Qt.rgba(0.02, 0.02, 0.04, 0.78 + slider * 0.16)
        if (kind === "frosted") return dark ? Qt.rgba(0.11, 0.11, 0.14, 0.28 + slider * 0.38) : Qt.rgba(1, 1, 1, 0.32 + slider * 0.38)
        return dark ? Qt.rgba(0.14, 0.15, 0.18, 0.165 + slider * 0.19) : Qt.rgba(1, 1, 1, 0.18 + slider * 0.24)
    }

    RectangularShadow {
        anchors.fill: body
        visible: g.shadow && !Theme.reduceTransparency
        radius: g.r
        blur: 40
        spread: -6
        offset.y: 10
        color: Qt.rgba(0, 0, 0, 0.15)
    }

    Rectangle {
        id: body
        anchors.fill: parent
        radius: g.r
        color: g.body
        border.width: Theme.showBorders ? 1.5 : 0
        border.color: Theme.rim
    }

    // Subtle curved-glass meniscus sheen along the top and bottom bevels.
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.kind === "smoke" ? 0.07 : (g.dark ? 0.12 : 0.22)) }
            GradientStop { position: 0.28; color: Qt.rgba(1, 1, 1, 0) }
            GradientStop { position: 0.78; color: Qt.rgba(1, 1, 1, 0) }
            GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.kind === "smoke" ? 0.06 : (g.dark ? 0.05 : 0.10)) }
        }
    }

    // Outer 1px specular rim: crisp top highlight + bottom bounce, quiet sides.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.62 : 0.92) }
                GradientStop { position: Math.min(0.42, 16 / Math.max(1, g.height)); color: Qt.rgba(1, 1, 1, g.dark ? 0.20 : 0.44) }
                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, g.dark ? 0.09 : 0.22) }
                GradientStop { position: Math.max(0.58, 1 - 16 / Math.max(1, g.height)); color: Qt.rgba(1, 1, 1, g.dark ? 0.16 : 0.34) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.42 : 0.68) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1; y: 1; width: Math.max(0, g.width - 2); height: Math.max(0, g.height - 2); radius: Math.max(0, g.r - 1) }
        }
    }

    // Inner 1px refractive counter-rim for physical glass-wall depth.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency && g.width > 8 && g.height > 8
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.14 : 0.28) }
                GradientStop { position: 0.5; color: Qt.rgba(0, 0, 0, g.dark ? 0.06 : 0.03) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.10 : 0.20) }
            }
            PathRectangle { x: 1; y: 1; width: Math.max(0, g.width - 2); height: Math.max(0, g.height - 2); radius: Math.max(0, g.r - 1) }
            PathRectangle { x: 2; y: 2; width: Math.max(0, g.width - 4); height: Math.max(0, g.height - 4); radius: Math.max(0, g.r - 2) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
