import QtQuick
import QtMultimedia

// Notchface: the webcam preview in the notch (loaded on demand, so a
// system without a camera or QtMultimedia never breaks the island).
Item {
    id: f
    property bool active: false
    CaptureSession {
        camera: Camera { id: cam; active: f.active }
        videoOutput: out
    }
    Rectangle { anchors.fill: parent; radius: 18; color: "#111" }
    VideoOutput { id: out; anchors.fill: parent; fillMode: VideoOutput.PreserveAspectCrop }
    Text {
        anchors.centerIn: parent
        visible: cam.error !== Camera.NoError || !cam.cameraDevice || cam.cameraDevice.isNull
        text: "No camera found"; color: Qt.rgba(1, 1, 1, 0.5); font.family: Theme.font; font.pixelSize: 13
    }
}
