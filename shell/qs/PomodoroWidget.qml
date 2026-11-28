import QtQuick
import Quickshell

// Exact getdroppy.app Pomodoro Timer Widget (Focus / Break pills, interactive
// tick-mark duration ruler 10..40 with amber active glow & center pointer,
// Start Timer pill, Sound & Auto-Break toggles, and large tabular countdown).
Item {
    id: p
    property var shell
    property int selectedMins: shell ? shell.pomodoroMinutes : 25
    property string phase: shell ? shell.pomodoroPhase : "focus"
    property bool soundOn: shell ? shell.pomodoroSound : true
    property bool autoBreak: shell ? shell.pomodoroAutoBreak : false
    readonly property int minVal: 5
    readonly property int maxVal: 45

    QtObject { id: ticker; property real now: Date.now() }
    readonly property bool running: shell !== null && shell.timerEnd > ticker.now
    readonly property int remainingSec: running ? Math.max(0, Math.ceil((shell.timerEnd - ticker.now) / 1000)) : selectedMins * 60

    Timer {
        interval: 500
        repeat: true
        running: p.visible && p.shell !== null && p.shell.timerEnd > 0
        onTriggered: ticker.now = Date.now()
    }

    function fmtClock(sec) {
        const m = Math.floor(sec / 60)
        const s = sec % 60
        return String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0")
    }

    function setMinutes(m) {
        const clamped = Math.max(minVal, Math.min(maxVal, Math.round(m)))
        selectedMins = clamped
        if (shell) {
            shell.pomodoroMinutes = clamped
            if (running) shell.timer(clamped * 60)
        }
    }

    function setPhase(ph) {
        phase = ph
        const defMins = ph === "break" ? 5 : 25
        selectedMins = defMins
        if (shell) {
            shell.pomodoroPhase = ph
            shell.pomodoroMinutes = defMins
            if (running) shell.timer(defMins * 60)
        }
    }

    function toggleStart() {
        if (!shell) return
        if (running) {
            shell.timer(0)
        } else {
            shell.pomodoroPhase = phase
            shell.pomodoroMinutes = selectedMins
            shell.timer(selectedMins * 60)
            ticker.now = Date.now()
        }
    }

    Column {
        anchors.fill: parent
        spacing: 12

        // Top row: Focus (amber) left, Break (dark/teal) right
        Item {
            width: parent.width
            height: 26
            Rectangle {
                anchors.left: parent.left
                height: 26; width: 64; radius: 13
                color: p.phase === "focus" ? "#452d0a" : Qt.rgba(1, 1, 1, 0.10)
                border.width: 1
                border.color: p.phase === "focus" ? Qt.rgba(1, 0.62, 0.04, 0.45) : Qt.rgba(1, 1, 1, 0.08)
                Text {
                    anchors.centerIn: parent
                    text: "Focus"
                    color: p.phase === "focus" ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.72)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                }
                TapHandler { onTapped: p.setPhase("focus") }
            }
            Rectangle {
                anchors.right: parent.right
                height: 26; width: 64; radius: 13
                color: p.phase === "break" ? "#113a2a" : Qt.rgba(1, 1, 1, 0.12)
                border.width: 1
                border.color: p.phase === "break" ? Qt.rgba(0.19, 0.82, 0.35, 0.45) : Qt.rgba(1, 1, 1, 0.08)
                Text {
                    anchors.centerIn: parent
                    text: "Break"
                    color: p.phase === "break" ? "#30d158" : Qt.rgba(1, 1, 1, 0.76)
                    font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold
                }
                TapHandler { onTapped: p.setPhase("break") }
            }
        }

        // Interactive Tick-Mark Duration Ruler Dial (10 15 20 25 30 35 40)
        Item {
            id: ruler
            width: parent.width
            height: 54
            readonly property color accentCol: p.phase === "break" ? "#30d158" : "#ff9f0a"
            readonly property int totalTicks: p.maxVal - p.minVal // 40 intervals -> 41 ticks

            // Numbers row (10, 15, 20, 25, 30, 35, 40)
            Repeater {
                model: [10, 15, 20, 25, 30, 35, 40]
                Text {
                    required property int modelData
                    readonly property real frac: (modelData - p.minVal) / ruler.totalTicks
                    readonly property bool active: Math.abs(modelData - p.selectedMins) <= 1
                    x: frac * (ruler.width - 12) + 6 - implicitWidth / 2
                    y: 0
                    text: modelData
                    color: active ? "white" : Qt.rgba(1, 0.72, 0.36, 0.56)
                    font.family: Theme.font
                    font.pixelSize: active ? 13 : 12
                    font.weight: active ? Font.Bold : Font.DemiBold
                    font.features: { "tnum": 1 }
                }
            }

            // Vertical tick bars
            Repeater {
                model: ruler.totalTicks + 1
                Rectangle {
                    required property int index
                    readonly property int val: p.minVal + index
                    readonly property bool major: val % 5 === 0
                    readonly property bool isSelected: val === p.selectedMins
                    readonly property bool filled: val < p.selectedMins
                    readonly property real frac: index / ruler.totalTicks
                    x: frac * (ruler.width - 12) + 6 - width / 2
                    y: isSelected ? 20 : (major ? 22 : 25)
                    width: isSelected ? 3 : 2
                    height: isSelected ? 24 : (major ? 20 : 14)
                    radius: 1
                    color: isSelected ? "white"
                         : filled ? Qt.rgba(ruler.accentCol.r, ruler.accentCol.g, ruler.accentCol.b, major ? 0.92 : 0.62)
                         : Qt.rgba(ruler.accentCol.r, ruler.accentCol.g, ruler.accentCol.b, major ? 0.38 : 0.22)
                }
            }

            // Crisp upward triangle indicator right beneath selected tick
            Text {
                readonly property real frac: (p.selectedMins - p.minVal) / ruler.totalTicks
                x: frac * (ruler.width - 12) + 6 - implicitWidth / 2
                y: 44
                text: "▲"
                color: ruler.accentCol
                font.pixelSize: 9
                font.weight: Theme.wBody
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                function pick(mx) {
                    const f = Math.max(0, Math.min(1, (mx - 6) / Math.max(1, ruler.width - 12)))
                    p.setMinutes(p.minVal + f * ruler.totalTicks)
                }
                onPressed: mouse => pick(mouse.x)
                onPositionChanged: mouse => { if (pressed) pick(mouse.x) }
            }
        }

        // Bottom controls row: Start Timer pill + Sound + Auto-break left; huge amber 25:00 right
        Item {
            width: parent.width
            height: 36

            Row {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8

                Rectangle {
                    height: 34; width: 108; radius: 17
                    color: p.phase === "break" ? "#113a2a" : "#452d0a"
                    border.width: 1
                    border.color: p.phase === "break" ? Qt.rgba(0.19, 0.82, 0.35, 0.45) : Qt.rgba(1, 0.62, 0.04, 0.42)
                    Text {
                        anchors.centerIn: parent
                        text: p.running ? "Stop Timer" : "Start Timer"
                        color: p.phase === "break" ? "#30d158" : "#ff9f0a"
                        font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                    }
                    TapHandler { onTapped: p.toggleStart() }
                }

                Rectangle {
                    width: 34; height: 34; radius: 17
                    color: p.soundOn ? "#452d0a" : Qt.rgba(1, 1, 1, 0.12)
                    Icon {
                        anchors.centerIn: parent
                        name: p.soundOn ? "volume" : "volumeMute"
                        size: 16
                        color: p.soundOn ? "#ff9f0a" : Qt.rgba(1, 1, 1, 0.65)
                    }
                    TapHandler {
                        onTapped: {
                            p.soundOn = !p.soundOn
                            if (p.shell) p.shell.pomodoroSound = p.soundOn
                        }
                    }
                }

                Rectangle {
                    width: 34; height: 34; radius: 17
                    color: p.autoBreak ? Qt.rgba(1, 1, 1, 0.24) : Qt.rgba(1, 1, 1, 0.12)
                    Icon {
                        anchors.centerIn: parent
                        name: "timer"
                        size: 16
                        color: p.autoBreak ? "white" : Qt.rgba(1, 1, 1, 0.72)
                    }
                    TapHandler {
                        onTapped: {
                            p.autoBreak = !p.autoBreak
                            if (p.shell) p.shell.pomodoroAutoBreak = p.autoBreak
                        }
                    }
                }
            }

            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: p.fmtClock(p.remainingSec)
                color: p.phase === "break" ? "#30d158" : "#ff9f0a"
                font.family: Theme.font
                font.pixelSize: 28
                font.weight: Font.Bold
                font.features: { "tnum": 1 }
            }
        }
    }
}
