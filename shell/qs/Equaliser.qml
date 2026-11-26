import QtQuick

// Five bars that dance while media plays (Droppy's orange visualiser).
Row {
    id: eq
    property bool active: false
    property color tint: "#ff9f0a"
    spacing: 2
    height: 14
    Repeater {
        model: 5
        Rectangle {
            required property int index
            width: 3; radius: 1.5; color: eq.tint
            anchors.bottom: parent.bottom
            height: eq.active ? 4 + Math.abs(Math.sin((t.frame + index * 7) / 4)) * 10 : 4
            Behavior on height { NumberAnimation { duration: 120 } }
        }
    }
    Timer { id: t; property int frame: 0; interval: 90; running: eq.active && !Theme.reduceMotion; repeat: true; onTriggered: frame++ }
}
