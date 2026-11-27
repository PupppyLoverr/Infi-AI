pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

// Solyra's look, shared by every surface. One glass value runs from ultra
// clear (0) to fully tinted (100); Reduce Transparency forces it opaque.
// Settings live in ~/.config/solyra/look.json and apply live.
Singleton {
    id: root

    property real glass: 35
    property bool reduceMotion: false
    property bool reduceTransparency: false
    property bool showBorders: false
    property bool dark: true
    property bool autoArrange: false
    property bool hideNotch: false
    property bool hotCorners: true         // bottom-left: Mission Control; bottom-right: desktop widgets peek
    property bool widgets: true            // desktop widgets (clock, calendar, weather)
    property bool nightShift: false        // warm display (hyprsunset)
    property bool dockMagnify: true        // macOS spring-damped Dock wave magnification
    property bool dockAutohide: false      // auto-hide Dock when pointer leaves

    // Tint alpha over the compositor's blur. Even at "ultra clear" the
    // floor keeps text readable (tested in tools/contrast).
    readonly property real tint: reduceTransparency ? 0.94 : 0.12 + glass / 100 * 0.78
    readonly property color glassFill: dark ? Qt.rgba(0.10, 0.10, 0.12, tint) : Qt.rgba(0.97, 0.97, 0.98, tint)
    readonly property color glassHi: dark ? Qt.rgba(1, 1, 1, 0.10) : Qt.rgba(1, 1, 1, 0.55)
    readonly property color rim: showBorders ? (dark ? "#b0ffffff" : "#90000000") : (dark ? Qt.rgba(1, 1, 1, 0.16) : Qt.rgba(0, 0, 0, 0.10))
    readonly property color ink: dark ? "#f5f5f7" : "#1d1d1f"
    readonly property color ink2: dark ? Qt.rgba(1, 1, 1, 0.62) : Qt.rgba(0, 0, 0, 0.55)
    readonly property color ink3: dark ? Qt.rgba(1, 1, 1, 0.38) : Qt.rgba(0, 0, 0, 0.32)
    readonly property color accent: "#0a84ff"
    readonly property color fill: dark ? Qt.rgba(1, 1, 1, 0.10) : Qt.rgba(0, 0, 0, 0.06)
    readonly property int radius: 16          // one window radius everywhere
    readonly property string font: "Inter"
    readonly property string mono: "JetBrains Mono"

    // Apple's island springs (Droppy uses response 0.24–0.4, damping 0.8):
    // QML SpringAnimation equivalents, or instant under Reduce Motion.
    readonly property real springOpen: 3.4
    readonly property real dampOpen: 0.30
    readonly property real springClose: 5.0
    readonly property real dampClose: 0.42
    function ms(v) { return reduceMotion ? 0 : v }

    readonly property string home: Quickshell.env("HOME")
    readonly property string iconDir: Qt.resolvedUrl("icons/")
    function icon(name) { return iconDir + name + ".svg" }

    FileView {
        id: file
        path: root.home + "/.config/solyra/look.json"
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                const j = JSON.parse(text())
                if (j.glass !== undefined) root.glass = j.glass
                if (j.reduceMotion !== undefined) root.reduceMotion = j.reduceMotion
                if (j.reduceTransparency !== undefined) root.reduceTransparency = j.reduceTransparency
                if (j.showBorders !== undefined) root.showBorders = j.showBorders
                if (j.dark !== undefined) root.dark = j.dark
                if (j.autoArrange !== undefined) root.autoArrange = j.autoArrange
                if (j.hideNotch !== undefined) root.hideNotch = j.hideNotch
                if (j.hotCorners !== undefined) root.hotCorners = j.hotCorners
                if (j.widgets !== undefined) root.widgets = j.widgets
                if (j.nightShift !== undefined) root.nightShift = j.nightShift
                if (j.dockMagnify !== undefined) root.dockMagnify = j.dockMagnify
                if (j.dockAutohide !== undefined) root.dockAutohide = j.dockAutohide
            } catch (e) {}
        }
    }

    function save() {
        file.setText(JSON.stringify({
            glass: glass, reduceMotion: reduceMotion, reduceTransparency: reduceTransparency,
            showBorders: showBorders, dark: dark, autoArrange: autoArrange, hideNotch: hideNotch,
            hotCorners: hotCorners, widgets: widgets, nightShift: nightShift,
            dockMagnify: dockMagnify, dockAutohide: dockAutohide
        }, null, 2))
    }
}
