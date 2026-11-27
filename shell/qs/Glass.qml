import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// macOS 27 Golden Gate Liquid Glass.
// Hyprland's acrylic blur variant handles the live GPU optics behind the
// surface (refraction lensing, curved bulb, vibrancy, subtle chromatic fringe),
// while this paints the optical glass surface on top:
//  - ultra-clean translucent glass body driven by the single 0..100 glass slider;
//  - soft top sky-sheen + subtle bottom internal bounce;
//  - directional dual-corner specular rim (brightest top-left, bounced
//    bottom-right, whisper-thin along flat edges);
//  - soft wide drop shadow kept below Hyprland's ignore_alpha (0.16) so the
//    shadow never turns into a blurred rectangle.
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
        blur: 36
        spread: -4
        offset.y: 10
        color: Qt.rgba(0, 0, 0, g.dark ? 0.15 : 0.11)
    }

    Rectangle {
        id: body
        anchors.fill: parent
        radius: g.r
        // Clearer floor (0.165 dark / 0.20 light) stays just above Hyprland's
        // ignore_alpha=0.16 so vibrant wallpapers refract cleanly through it.
        color: Theme.reduceTransparency
            ? (g.dark ? "#f01c1c20" : "#f6f6f6f8")
            : (g.dark ? Qt.rgba(0.08, 0.09, 0.12, 0.165 + Theme.glass / 100 * 0.48)
                      : Qt.rgba(1, 1, 1, 0.20 + Theme.glass / 100 * 0.48))
        border.width: Theme.showBorders ? 1.5 : 0
        border.color: Theme.rim
    }

    // Subtle top sky reflection + faint bottom internal caustic bounce
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop { position: 0.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.09 : 0.24) }
            GradientStop { position: 0.38; color: Qt.rgba(1, 1, 1, 0.0) }
            GradientStop { position: 0.82; color: Qt.rgba(1, 1, 1, 0.0) }
            GradientStop { position: 1.0;  color: Qt.rgba(0.85, 0.92, 1.0, g.dark ? 0.04 : 0.10) }
        }
    }

    // Directional dual-corner specular rim + inner luminous refraction bevel
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        // Outer crisp 1.2 px directional specular rim
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.56 : 0.92) }
                GradientStop { position: 0.22; color: Qt.rgba(1, 1, 1, g.dark ? 0.20 : 0.46) }
                GradientStop { position: 0.50; color: Qt.rgba(1, 1, 1, g.dark ? 0.09 : 0.24) }
                GradientStop { position: 0.78; color: Qt.rgba(1, 1, 1, g.dark ? 0.18 : 0.40) }
                GradientStop { position: 1.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.40 : 0.76) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1.2; y: 1.2; width: Math.max(0, g.width - 2.4); height: Math.max(0, g.height - 2.4); radius: Math.max(0, g.r - 1.2) }
        }
        // Soft 2 px inner luminous caustic ring (macOS 27 curved lens edge)
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.16 : 0.34) }
                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, 0.0) }
                GradientStop { position: 1.0; color: Qt.rgba(0.85, 0.92, 1.0, g.dark ? 0.12 : 0.24) }
            }
            PathRectangle { x: 1.2; y: 1.2; width: Math.max(0, g.width - 2.4); height: Math.max(0, g.height - 2.4); radius: Math.max(0, g.r - 1.2) }
            PathRectangle { x: 3.4; y: 3.4; width: Math.max(0, g.width - 6.8); height: Math.max(0, g.height - 6.8); radius: Math.max(0, g.r - 3.4) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
