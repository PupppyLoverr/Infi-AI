import QtQuick

// Six bars that dance while media plays (Droppy's coral/amber visualiser).
Row {
    id: eq
    property bool active: false
    property color tint: "#ff7a45"
    spacing: 2
    height: 14
    Repeater {
        model: 6
        Rectangle {
            required property int index
            width: 2; radius: 1; color: eq.tint
            anchors.verticalCenter: parent.verticalCenter
            height: eq.active ? 4 + Math.abs(Math.sin((t.frame + index * 6) / 3.6)) * 10 : 4
            Behavior on height { NumberAnimation { duration: 110 } }
        }
    }
    Timer { id: t; property int frame: 0; interval: 85; running: eq.active && !Theme.reduceMotion; repeat: true; onTriggered: frame++ }
}
