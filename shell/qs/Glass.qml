import QtQuick
import QtQuick.Shapes
import QtQuick.Effects

// Exact Cupertino (orafaelfragoso/cupertino) + macOS 27 Golden Gate Liquid Glass.
// Hyprland's acrylic lens bends the backdrop at each surface's curved edge
// (ignore_alpha 0.16, so only the card's own shape is glass). This implements
// Cupertino's five optical material recipes from cupertino-theme/src/glass/:
//  - "module"  Control Center tiles (fitted.rs::control_center): mid-tone luma
//              compression face, equal top+bottom bright Line of Light, 6px inner
//              diffuse glow, and dark side edge hairlines (zero on top/bottom).
//  - "dock"    Dock bar (fitted.rs::dock_bar + clear.rs::dock): vibrant clear
//              face with dark luma cap, 1.5px top+bottom specular light + 8px
//              diffuse glow, and dark curved-end outer rim.
//  - "clear"   Floating capsules & pills (clear.rs::clear_base).
//  - "frosted" Spotlight results, notifications, menus (regular.rs::regular_base):
//              denser face, inner top ring shadow, key+fill specular arcs.
//  - "smoke"   Spotlight search capsule: dark smoked glass.
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
    readonly property bool isModule: kind === "module"
    readonly property bool isDock: kind === "dock"
    readonly property bool isFrosted: kind === "frosted"
    readonly property bool isSmoke: kind === "smoke"

    // Stage 2: Face fill + luma compression (matches Cupertino's MODULE_WHITE=0.685,
    // MODULE_BLACK=0.107, DOCK_BAR_MAX_LUMA_DARK=0.75, REGULAR_FILL_LIGHT=0.20).
    readonly property color body: {
        if (Theme.reduceTransparency) return dark ? "#f21c1c1f" : "#f6f6f6f8"
        if (isSmoke) return Qt.rgba(0.02, 0.02, 0.04, 0.78 + slider * 0.16)
        if (isModule) {
            // Cupertino Control Center modules are appearance-invariant mid-toned
            // glass (0.59 * c + 27) so white text/icons read cleanly on sunlit walls.
            return Qt.rgba(0.105, 0.11, 0.125, 0.24 + slider * 0.20)
        }
        if (isDock) {
            return dark
                ? Qt.rgba(0.11, 0.12, 0.145, 0.19 + slider * 0.17)
                : Qt.rgba(0.98, 0.99, 1.0, 0.21 + slider * 0.20)
        }
        if (isFrosted) {
            return dark
                ? Qt.rgba(0.105, 0.11, 0.135, 0.29 + slider * 0.36)
                : Qt.rgba(1, 1, 1, 0.34 + slider * 0.36)
        }
        return dark
            ? Qt.rgba(0.13, 0.14, 0.17, 0.17 + slider * 0.18)
            : Qt.rgba(1, 1, 1, 0.19 + slider * 0.22)
    }

    // Stage 8a: Soft ambient drop shadow (stays below ignore_alpha=0.16).
    RectangularShadow {
        anchors.fill: body
        visible: g.shadow && !Theme.reduceTransparency
        radius: g.r
        blur: 42
        spread: -6
        offset.y: 10
        color: Qt.rgba(0, 0, 0, 0.15)
    }

    // Stage 1-2: Base refracted glass body.
    Rectangle {
        id: body
        anchors.fill: parent
        radius: g.r
        color: g.body
        border.width: Theme.showBorders ? 1.5 : 0
        border.color: Theme.rim
    }

    // Stage 5b: Cupertino inner diffuse glow (MODULE_LINE_DIFFUSE height_scale=6,
    // amount_scale=0.2; DOCK_BAR_LIGHT_DIFFUSE 8px deep; SPECULAR_DIFFUSE 0.15).
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency
        gradient: Gradient {
            GradientStop {
                position: 0.0
                color: Qt.rgba(1, 1, 1, g.isSmoke ? 0.07 : (g.isModule ? 0.15 : (g.isDock ? (g.dark ? 0.16 : 0.26) : (g.dark ? 0.12 : 0.22))))
            }
            GradientStop {
                position: Math.min(0.24, (g.isDock ? 9 : 7) / Math.max(1, g.height))
                color: Qt.rgba(1, 1, 1, 0)
            }
            GradientStop {
                position: Math.max(0.76, 1 - (g.isDock ? 9 : 7) / Math.max(1, g.height))
                color: Qt.rgba(1, 1, 1, 0)
            }
            GradientStop {
                position: 1.0
                color: Qt.rgba(1, 1, 1, g.isSmoke ? 0.06 : (g.isModule ? 0.13 : (g.isDock ? (g.dark ? 0.13 : 0.20) : (g.dark ? 0.06 : 0.12))))
            }
        }
    }

    // Stage 7: Cupertino inner ring shadow (REGULAR_RING_OPACITY=0.06, offset=8px)
    // softly shades just below the top edge on frosted panels.
    Rectangle {
        anchors.fill: parent
        radius: g.r
        visible: !Theme.reduceTransparency && g.isFrosted && g.height > 24
        gradient: Gradient {
            GradientStop { position: 0.0; color: Qt.rgba(0, 0, 0, 0) }
            GradientStop { position: Math.min(0.18, 10 / Math.max(1, g.height)); color: Qt.rgba(0, 0, 0, g.dark ? 0.06 : 0.03) }
            GradientStop { position: Math.min(0.36, 22 / Math.max(1, g.height)); color: Qt.rgba(0, 0, 0, 0) }
        }
    }

    // Stage 5a: Cupertino Top & Bottom Line of Light (MODULE_LINE=0.65 top & bottom,
    // curvature=0.5 wrapping corners; DOCK_BAR_LIGHT=1.0 at 1.5px depth; quiet sides).
    Shape {
        id: rimShape
        anchors.fill: parent
        visible: !Theme.reduceTransparency
        preferredRendererType: Shape.CurveRenderer
        readonly property real rimW: g.isDock ? 1.4 : 1.15
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop {
                    position: 0.0
                    color: Qt.rgba(1, 1, 1, g.isModule ? 0.72 : (g.isDock ? (g.dark ? 0.76 : 0.94) : (g.dark ? 0.58 : 0.88)))
                }
                GradientStop {
                    position: Math.min(0.38, 18 / Math.max(1, g.height))
                    color: Qt.rgba(1, 1, 1, g.isModule ? 0.24 : (g.isDock ? 0.22 : (g.dark ? 0.18 : 0.38)))
                }
                GradientStop {
                    position: 0.5
                    color: Qt.rgba(1, 1, 1, g.isModule ? 0.04 : (g.isDock ? 0.05 : (g.dark ? 0.08 : 0.18)))
                }
                GradientStop {
                    position: Math.max(0.62, 1 - 18 / Math.max(1, g.height))
                    color: Qt.rgba(1, 1, 1, g.isModule ? 0.22 : (g.isDock ? 0.20 : (g.dark ? 0.15 : 0.32)))
                }
                GradientStop {
                    position: 1.0
                    color: Qt.rgba(1, 1, 1, g.isModule ? 0.66 : (g.isDock ? (g.dark ? 0.64 : 0.84) : (g.dark ? 0.42 : 0.66)))
                }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle {
                x: rimShape.rimW; y: rimShape.rimW
                width: Math.max(0, g.width - 2 * rimShape.rimW)
                height: Math.max(0, g.height - 2 * rimShape.rimW)
                radius: Math.max(0, g.r - rimShape.rimW)
            }
        }
    }

    // Stage 8b: Cupertino Signature Dark Side Edge Hairline (MODULE_EDGE_LINE=0.64,
    // MODULE_EDGE_LINE_SIDES=PI/2; DOCK_BAR_RIM_SPREAD=120°): dark ONLY on the
    // left/right vertical arcs, zero along the top/bottom specular lines.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency && (g.isModule || g.isDock) && g.width > 12 && g.height > 12
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(0, 0, 0, 0.0) }
                GradientStop { position: Math.min(0.28, 14 / Math.max(1, g.height)); color: Qt.rgba(0, 0, 0, g.isModule ? 0.22 : 0.16) }
                GradientStop { position: 0.5; color: Qt.rgba(0, 0, 0, g.isModule ? 0.42 : 0.30) }
                GradientStop { position: Math.max(0.72, 1 - 14 / Math.max(1, g.height)); color: Qt.rgba(0, 0, 0, g.isModule ? 0.22 : 0.16) }
                GradientStop { position: 1.0; color: Qt.rgba(0, 0, 0, 0.0) }
            }
            PathRectangle { x: 0; y: 0; width: g.width; height: g.height; radius: g.r }
            PathRectangle { x: 1; y: 1; width: Math.max(0, g.width - 2); height: Math.max(0, g.height - 2); radius: Math.max(0, g.r - 1) }
        }
    }

    // Stage 4: Delicate inner refractive counter-rim for physical glass-wall depth.
    Shape {
        anchors.fill: parent
        visible: !Theme.reduceTransparency && g.width > 8 && g.height > 8
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeWidth: 0
            fillRule: ShapePath.OddEvenFill
            fillGradient: LinearGradient {
                x1: 0; y1: 0; x2: 0; y2: g.height
                GradientStop { position: 0.0; color: Qt.rgba(1, 1, 1, g.isModule ? 0.18 : (g.dark ? 0.14 : 0.26)) }
                GradientStop { position: 0.5; color: Qt.rgba(0, 0, 0, g.dark ? 0.08 : 0.04) }
                GradientStop { position: 1.0; color: Qt.rgba(1, 1, 1, g.isModule ? 0.14 : (g.dark ? 0.10 : 0.18)) }
            }
            PathRectangle { x: 1; y: 1; width: Math.max(0, g.width - 2); height: Math.max(0, g.height - 2); radius: Math.max(0, g.r - 1) }
            PathRectangle { x: 2; y: 2; width: Math.max(0, g.width - 4); height: Math.max(0, g.height - 4); radius: Math.max(0, g.r - 2) }
        }
    }

    Item { id: holder; anchors.fill: parent }
}
