import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// Liquid Glass, after macOS 26/27 (as photographed on a MacBook Pro, Nov 2026).
// Hyprland does the optics behind the surface: a strong blur, vibrancy and the
// acrylic lens that bends the backdrop through the curved edge (layer rules
// with ignore_alpha 0.16, so only the card's own shape is glass). This draws
// the material on top, in three kinds:
//  - "clear"   Control Center tiles, the dock, menus: the backdrop stays vivid,
//              a breath of tint;
//  - "frosted" Spotlight's results, notifications: a little more body so text
//              sits on a soft, colour-washed blur;
//  - "smoke"   Spotlight's search capsule: dark, smoky glass.
// Every kind has the thin specular rim Apple's glass shows, lit along the top
// edge and again along the bottom, dim at the sides, and a deep soft shadow.
// Bodies stay above the compositor's ignore_alpha so they are always blurred;
// shadows stay below it so they never are. Reduce Transparency swaps it all
// for a solid fill.
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
        if (kind === "smoke") return Qt.rgba(0.02, 0.02, 0.04, 0.80 + slider * 0.15)
        if (kind === "frosted") return dark ? Qt.rgba(0.12, 0.12, 0.14, 0.30 + slider * 0.40) : Qt.rgba(1, 1, 1, 0.34 + slider * 0.40)
        return dark ? Qt.rgba(0.18, 0.18, 0.20, 0.17 + slider * 0.22) : Qt.rgba(1, 1, 1, 0.20 + slider * 0.30)
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

    // The curved glass catching light: a soft lift across the top third.
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.kind === "smoke" ? 0.05 : 0.08) }
            GradientStop { position: 0.35; color: Qt.rgba(1, 1, 1, 0) }
            GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.kind === "smoke" ? 0.06 : 0.03) }
        }
    }

    // Specular rim: a 1 px ring, bright along the top, fading down the sides,
    // and lit again (less) along the bottom where the light exits the lens.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.50 : 0.85) }
                GradientStop { position: Math.min(0.45, 18 / Math.max(1, g.height)); color: Qt.rgba(1, 1, 1, g.dark ? 0.16 : 0.40) }
                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, g.dark ? 0.08 : 0.22) }
                GradientStop { position: Math.max(0.55, 1 - 18 / Math.max(1, g.height)); color: Qt.rgba(1, 1, 1, g.dark ? 0.12 : 0.30) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.34 : 0.60) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1; y: 1; width: g.width - 2; height: g.height - 2; radius: Math.max(0, g.r - 1) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
