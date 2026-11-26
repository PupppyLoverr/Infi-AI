import QtQuick
import QtQuick.Effects

// A Solar glyph tinted to `color`.
Item {
    id: i
    property string name
    property color color: Theme.ink
    property int size: 18
    width: size; height: size
    // The glyphs are white; only tinted ones pay for an effect layer.
    readonly property bool tinted: !Qt.colorEqual(i.color, "white") && !Qt.colorEqual(i.color, Theme.ink)
    Image {
        id: img
        anchors.fill: parent
        source: Theme.icon(i.name)
        sourceSize: Qt.size(i.size * 2, i.size * 2)
        visible: !i.tinted
        opacity: i.color.a
    }
    Loader {
        anchors.fill: parent
        active: i.tinted
        sourceComponent: MultiEffect {
            source: img
            colorization: 1
            colorizationColor: i.color
        }
    }
}
