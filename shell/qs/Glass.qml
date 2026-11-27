import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// Liquid Glass. Hyprland blurs and saturates what is behind the surface
// (layer rule with ignore_alpha, so only the card's shape is frosted); this
// draws Apple's material on top of that backdrop:
//  - a clear, slightly tinted body (Theme.glass: ultra clear -> tinted);
//  - the lensing rim: a bright edge ring, strongest top-left where light
//    falls, fading round the shape;
//  - a specular sheen across the top and a soft inner shade at the bottom;
//  - a soft two-layer drop shadow lifting the card off the desktop.
Item {
    id: g
    property real r: Theme.radius
    property bool dark: Theme.dark
    property bool shadow: true
    default property alias content: holder.data

    // Soft drop shadow lifting the card (skipped under Reduce Transparency).
    RectangularShadow {
        anchors.fill: body
        visible: g.shadow && !Theme.reduceTransparency
        radius: g.r
        blur: 28
        spread: 0
        offset.y: 10
        color: Qt.rgba(0, 0, 0, g.dark ? 0.38 : 0.20)
    }

    Rectangle {
        id: body
        anchors.fill: parent
        radius: g.r
        // Clear body: a whisper of white over a dark scene, more as the
        // glass slider moves toward "tinted".
        color: Theme.reduceTransparency
            ? (g.dark ? "#ee1c1c20" : "#f4f6f6f8")
            : (g.dark ? Qt.rgba(0.11, 0.11, 0.13, 0.06 + Theme.glass / 100 * 0.58)
                      : Qt.rgba(1, 1, 1, 0.14 + Theme.glass / 100 * 0.56))
        border.width: Theme.showBorders ? 1.5 : 0
        border.color: Theme.rim
    }

    // Specular sheen across the top (light from above).
    Rectangle {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: Math.min(parent.height * 0.55, 46)
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.16 : 0.42) }
            GradientStop { position: 1.0; color: "transparent" }
        }
    }
    // Inner shade along the bottom edge (glass thickness).
    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: Math.min(parent.height * 0.35, 26)
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0; color: "transparent" }
            GradientStop { position: 1.0; color: Qt.rgba(0, 0, 0, g.dark ? 0.16 : 0.06) }
        }
    }

    // The lensing rim: a ring (outer rounded rect minus an inner one) filled
    // with a diagonal gradient, bright top-left, dim bottom-right; a second
    // faint ring inside gives the edge its thickness.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.55 : 0.85) }
                GradientStop { position: 0.35; color: Qt.rgba(1, 1, 1, g.dark ? 0.14 : 0.35) }
                GradientStop { position: 0.65; color: Qt.rgba(1, 1, 1, g.dark ? 0.08 : 0.25) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.30 : 0.6) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1.2; y: 1.2; width: g.width - 2.4; height: g.height - 2.4; radius: Math.max(0, g.r - 1.2) }
        }
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillColor: Qt.rgba(1, 1, 1, g.dark ? 0.05 : 0.12)
            PathRectangle { x: 2.4; y: 2.4; width: g.width - 4.8; height: g.height - 4.8; radius: Math.max(0, g.r - 2.4) }
            PathRectangle { x: 4; y: 4; width: g.width - 8; height: g.height - 8; radius: Math.max(0, g.r - 4) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
