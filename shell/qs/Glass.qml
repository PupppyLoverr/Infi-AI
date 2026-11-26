import QtQuick

// A Liquid Glass card. The compositor blurs and lenses what is behind it
// (Hyprland layer rule + the solyra-glass plugin); this draws the tint, the
// rim light and the top specular highlight.
Rectangle {
    id: g
    property real r: Theme.radius
    radius: r
    color: Theme.glassFill
    border.width: 1
    border.color: Theme.rim
    Rectangle {
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: 1 }
        height: Math.min(parent.height / 2, 28)
        radius: g.r
        gradient: Gradient {
            GradientStop { position: 0; color: Theme.glassHi }
            GradientStop { position: 1; color: "transparent" }
        }
    }
}
