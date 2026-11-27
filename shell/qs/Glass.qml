import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// macOS 27 Golden Gate Liquid Glass.
// Hyprland's acrylic blur variant handles the live GPU optics behind the
// surface (refraction lensing, curved bulb, vibrancy, subtle chromatic fringe),
// while this paints the exact macOS 27 optical glass surface on top:
//  - ultra-clean translucent glass body driven by the single 0..100 glass slider;
//  - soft top sky-sheen + subtle bottom internal bounce;
//  - directional dual-corner specular rim (brightest top-left, bounced
//    bottom-right, whisper-thin along flat edges);
//  - upper-right curved glass specular caustic arc (macOS 27 pill refraction);
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
            GradientStop { position: 0.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.11 : 0.28) }
            GradientStop { position: 0.36; color: Qt.rgba(1, 1, 1, 0.0) }
            GradientStop { position: 0.80; color: Qt.rgba(1, 1, 1, 0.0) }
            GradientStop { position: 1.0;  color: Qt.rgba(0.86, 0.93, 1.0, g.dark ? 0.05 : 0.12) }
        }
    }

    // Directional dual-corner specular rim + inner luminous refraction bevel
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        // Outer crisp 1.25 px directional specular rim
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.62 : 0.95) }
                GradientStop { position: 0.22; color: Qt.rgba(1, 1, 1, g.dark ? 0.22 : 0.50) }
                GradientStop { position: 0.50; color: Qt.rgba(1, 1, 1, g.dark ? 0.10 : 0.26) }
                GradientStop { position: 0.78; color: Qt.rgba(1, 1, 1, g.dark ? 0.22 : 0.44) }
                GradientStop { position: 1.0;  color: Qt.rgba(1, 1, 1, g.dark ? 0.48 : 0.82) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1.25; y: 1.25; width: Math.max(0, g.width - 2.5); height: Math.max(0, g.height - 2.5); radius: Math.max(0, g.r - 1.25) }
        }
        // Soft 2.2 px inner luminous caustic ring (macOS 27 curved lens edge)
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: g.width; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.dark ? 0.19 : 0.38) }
                GradientStop { position: 0.45; color: Qt.rgba(1, 1, 1, 0.0) }
                GradientStop { position: 1.0; color: Qt.rgba(0.86, 0.93, 1.0, g.dark ? 0.15 : 0.28) }
            }
            PathRectangle { x: 1.25; y: 1.25; width: Math.max(0, g.width - 2.5); height: Math.max(0, g.height - 2.5); radius: Math.max(0, g.r - 1.25) }
            PathRectangle { x: 3.5; y: 3.5; width: Math.max(0, g.width - 7.0); height: Math.max(0, g.height - 7.0); radius: Math.max(0, g.r - 3.5) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
