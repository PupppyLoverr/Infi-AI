import QtQuick
import Quickshell
import Quickshell.Wayland

// The desktop picture, drawn by the shell on the background layer (one less
// process than swaybg). ~/.config/solyra/wallpaper.png, cropped to fill.
PanelWindow {
    property var screenModel
    screen: screenModel
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: "#101418"
    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "solyra-wallpaper"
    Image {
        anchors.fill: parent
        source: "file://" + Theme.home + "/.config/solyra/wallpaper.png"
        fillMode: Image.PreserveAspectCrop
        sourceSize: Qt.size(parent.width, parent.height)
        asynchronous: true
        cache: false
    }
}
