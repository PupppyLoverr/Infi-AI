import QtQuick
import QtQuick.Effects

// A Solar glyph tinted to `color`.
Item {
    id: i
    property string name
    property color color: Theme.ink
    property int size: 18
    width: size; height: size
    Image {
        id: img
        anchors.fill: parent
        source: Theme.icon(i.name)
        sourceSize: Qt.size(i.size * 2, i.size * 2)
        visible: false
    }
    MultiEffect {
        anchors.fill: parent
        source: img
        colorization: 1
        colorizationColor: i.color
    }
}
