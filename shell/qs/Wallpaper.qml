import QtQuick
import Quickshell
import Quickshell.Wayland

// The desktop picture, drawn by the shell on the background layer (one less
// process than swaybg). Uses Theme.wallpaperPath if set, otherwise
// ~/.config/solyra/wallpaper.jpg (falling back to wallpaper.png), cropped to fill
// and decoded at screen size to keep RAM low.
PanelWindow {
    property var screenModel
    screen: screenModel
    anchors { top: true; bottom: true; left: true; right: true }
    exclusionMode: ExclusionMode.Ignore
    color: "#101418"
    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "solyra-wallpaper"
    Image {
        id: img
        property bool fallbackPng: false
        anchors.fill: parent
        source: Theme.wallpaperPath !== ""
            ? ("file://" + Theme.wallpaperPath)
            : ("file://" + Theme.home + "/.config/solyra/" + (fallbackPng ? "wallpaper.png" : "wallpaper.jpg"))
        fillMode: Image.PreserveAspectCrop
        sourceSize: Qt.size(Math.max(1, parent.width), Math.max(1, parent.height))
        asynchronous: true
        cache: false
        onStatusChanged: {
            if (status === Image.Error && Theme.wallpaperPath === "" && !fallbackPng) fallbackPng = true
        }
    }
}
