import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// Liquid Glass, after macOS 26/27. Hyprland does the optics behind the
// surface: blur, saturation and the acrylic lens that bends the backdrop
// through the curved edge (layer rules with ignore_alpha, so only the card's
// shape is glass). This draws the material on top:
//  - a nearly clear body: a breath of tint, more as the glass slider moves
//    toward "tinted";
//  - the specular rim: a thin bright edge where light catches the curve,
//    strongest at the top-left and, fainter, at the bottom-right (Apple's
//    highlight sits on opposite corners), dim along the sides;
//  - a light, wide shadow that lifts the card without darkening the desktop.
// Reduce Transparency swaps all of it for a solid, legible fill.
Item {
    id: g
    property real r: Theme.radius
    property bool dark: Theme.dark
    property bool shadow: true
    default property alias content: holder.data

    RectangularShadow {
        anchors.fill: body
        visible: g.shadow && !Theme.reduceTransparency
        radius: g.r
        blur: 32
        spread: -4
        offset.y: 8
        // Kept under the compositor's ignore_alpha (0.16) so the shadow is
        // never blurred into a box.
        color: Qt.rgba(0, 0, 0, g.dark ? 0.15 : 0.10)
    }

    Rectangle {
        id: body
        anchors.fill: parent
        radius: g.r
        color: Theme.reduceTransparency
            ? (g.dark ? "#f01c1c20" : "#f6f6f6f8")
            : (g.dark ? Qt.rgba(0.09, 0.09, 0.11, 0.18 + Theme.glass / 100 * 0.46)
                      : Qt.rgba(1, 1, 1, 0.20 + Theme.glass / 100 * 0.46))
        border.width: Theme.showBorders ? 1.5 : 0
        border.color: Theme.rim
    }

    // A whisper of light across the upper half (the curved top catching the
    // sky), not a band.
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.07 : 0.20) }
            GradientStop { position: 0.45; color: Qt.rgba(1, 1, 1, 0) }
        }
    }

    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        // Specular rim: a 1 px ring lit from the top-left, echoed bottom-right.
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.42 : 0.85) }
                GradientStop { position: 0.25; color: Qt.rgba(1, 1, 1, g.dark ? 0.16 : 0.40) }
                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, g.dark ? 0.09 : 0.25) }
                GradientStop { position: 0.75; color: Qt.rgba(1, 1, 1, g.dark ? 0.14 : 0.35) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.30 : 0.65) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1; y: 1; width: g.width - 2; height: g.height - 2; radius: Math.max(0, g.r - 1) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
