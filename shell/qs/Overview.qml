import QtQuick
import Quickshell
import Quickshell.Wayland

// Mission Control: every window as a live thumbnail (screencopy of the
// toplevel), laid out in a grid that zooms in from the windows' places.
// Click a window to bring it forward; Esc or a click on the desktop closes.
PanelWindow {
    id: ov
    property bool open: false
    visible: open || fade.running
    anchors { top: true; bottom: true; left: true; right: true }
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-overview"
    WlrLayershell.keyboardFocus: open ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    readonly property var wins: ToplevelManager.toplevels.values.filter(t => !t.minimized)
    readonly property int cols: Math.max(1, Math.ceil(Math.sqrt(wins.length)))
    readonly property int rows: Math.max(1, Math.ceil(wins.length / cols))

    Rectangle {
        id: dim
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.35)
        opacity: ov.open ? 1 : 0
        Behavior on opacity { NumberAnimation { id: fade; duration: Theme.ms(260); easing.type: Easing.OutCubic } }
        MouseArea { anchors.fill: parent; onClicked: ov.open = false }
    }
    Item {
        anchors.fill: parent
        focus: ov.open
        Keys.onEscapePressed: ov.open = false
    }
    Text {
        visible: ov.wins.length === 0
        anchors.centerIn: parent
        text: "No open windows"; color: "white"; opacity: dim.opacity; font.family: Theme.font; font.pixelSize: 18
    }
    Repeater {
        model: ov.wins
        Item {
            id: cell
            required property var modelData
            required property int index
            readonly property real cw: (ov.width - 120) / ov.cols
            readonly property real ch: (ov.height - 200) / ov.rows
            readonly property int c: index % ov.cols
            readonly property int r: Math.floor(index / ov.cols)
            x: ov.open ? 60 + c * cw + 12 : ov.width / 2 - width / 2
            y: ov.open ? 100 + r * ch + 12 : ov.height / 2 - height / 2
            width: cw - 24; height: ch - 40
            scale: ov.open ? 1 : 0.6
            opacity: ov.open ? 1 : 0
            Behavior on x { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen } }
            Behavior on y { enabled: !Theme.reduceMotion; SpringAnimation { spring: Theme.springOpen; damping: Theme.dampOpen } }
            Behavior on scale { NumberAnimation { duration: Theme.ms(280); easing.type: Easing.OutCubic } }
            Behavior on opacity { NumberAnimation { duration: Theme.ms(200) } }
            Rectangle {
                anchors.fill: shot; anchors.margins: -3; radius: Theme.radius + 3
                color: "transparent"; border.width: 3
                border.color: hov.hovered ? Theme.accent : "transparent"
            }
            ScreencopyView {
                id: shot
                // Keep the window's aspect so the highlight hugs the thumbnail.
                readonly property real k: sourceSize.width > 0 && sourceSize.height > 0
                    ? Math.min(parent.width / sourceSize.width, parent.height / sourceSize.height) : 1
                anchors.centerIn: parent
                width: sourceSize.width > 0 ? sourceSize.width * k : parent.width
                height: sourceSize.height > 0 ? sourceSize.height * k : parent.height
                captureSource: ov.open ? cell.modelData : null
                live: ov.open
                layer.enabled: true
            }
            Text {
                anchors.top: shot.bottom; anchors.topMargin: 10
                anchors.horizontalCenter: parent.horizontalCenter
                width: parent.width; horizontalAlignment: Text.AlignHCenter; elide: Text.ElideRight
                text: cell.modelData.title || cell.modelData.appId
                color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Medium
            }
            HoverHandler { id: hov }
            TapHandler { onTapped: { cell.modelData.activate(); ov.open = false } }
        }
    }
}
