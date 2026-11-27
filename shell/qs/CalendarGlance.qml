import QtQuick
import QtQuick.Controls

// Exact getdroppy.app Two-Column Calendar Glance:
// Left column: Red `SUN` + white `25` + muted ISO week `W44` + circular `+` &
// expand buttons, stacked translucent colored event cards with crisp left bars
// (plus inline quick event creator!).
// Right column: Red `OCT` + 7-column `S M T W T F S` month grid with solid red
// circle highlighting today.
Item {
    id: c
    property var shell
    property bool addingEvent: false
    readonly property date now: new Date()
    readonly property int first: new Date(now.getFullYear(), now.getMonth(), 1).getDay()
    readonly property int days: new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate()
    readonly property int isoWeek: {
        const d = new Date(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()))
        const dayNum = d.getUTCDay() || 7
        d.setUTCDate(d.getUTCDate() + 4 - dayNum)
        const yearStart = new Date(Date.UTC(d.getUTCFullYear(), 0, 1))
        return Math.ceil((((d - yearStart) / 86400000) + 1) / 7)
    }

    Row {
        anchors.fill: parent
        spacing: 20

        // Left column: Day header + events list
        Column {
            width: parent.width * 0.48
            height: parent.height
            spacing: 9

            Item {
                width: parent.width
                height: 24
                Row {
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 5
                    Text { text: Qt.formatDate(c.now, "ddd").toUpperCase(); color: "#ff453a"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold }
                    Text { text: c.now.getDate(); color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold }
                    Text { text: "W" + c.isoWeek; color: Qt.rgba(1, 1, 1, 0.42); font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold }
                }
                Row {
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 6
                    Rectangle {
                        width: 22; height: 22; radius: 11
                        color: c.addingEvent ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.14)
                        Icon { anchors.centerIn: parent; name: "plus"; size: 11; color: "white" }
                        TapHandler {
                            onTapped: {
                                c.addingEvent = !c.addingEvent
                                if (c.addingEvent) evInput.forceActiveFocus()
                            }
                        }
                    }
                    Rectangle {
                        width: 22; height: 22; radius: 11
                        color: Qt.rgba(1, 1, 1, 0.14)
                        Icon { anchors.centerIn: parent; name: "expand"; size: 11; color: Qt.rgba(1, 1, 1, 0.85) }
                    }
                }
            }

            // Inline Quick-Add Event bar when `+` is clicked
            Rectangle {
                visible: c.addingEvent
                width: parent.width; height: 30; radius: 9
                color: Qt.rgba(1, 1, 1, 0.10)
                border.width: 1; border.color: Qt.rgba(0.04, 0.52, 1.0, 0.65)
                TextField {
                    id: evInput
                    anchors.fill: parent; anchors.leftMargin: 8; anchors.rightMargin: 8
                    placeholderText: "New event (Enter to add)…"
                    color: "white"; placeholderTextColor: Qt.rgba(1, 1, 1, 0.44)
                    font.family: Theme.font; font.pixelSize: 11
                    background: null
                    onAccepted: {
                        if (c.shell && text.trim()) c.shell.addCalendarEvent(text, Qt.formatDateTime(new Date(), "HH:mm"))
                        text = ""
                        c.addingEvent = false
                    }
                }
            }

            // Translucent colored event cards with crisp left stripe
            Column {
                width: parent.width
                spacing: 8
                Repeater {
                    model: c.shell ? c.shell.calendarEvents.slice(0, c.addingEvent ? 2 : 3) : []
                    Rectangle {
                        required property var modelData
                        width: parent.width
                        height: 44
                        radius: 10
                        color: modelData.bg || "#201c3a"
                        border.width: 1
                        border.color: Qt.rgba(1, 1, 1, 0.08)
                        Rectangle {
                            x: 7; y: 7; width: 3; height: parent.height - 14; radius: 1.5
                            color: modelData.accent || "#8e82ff"
                        }
                        Column {
                            anchors { left: parent.left; leftMargin: 16; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
                            spacing: 2
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: modelData.title
                                color: modelData.accent || "#b7adff"
                                font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold
                            }
                            Text {
                                width: parent.width; elide: Text.ElideRight
                                text: modelData.time
                                color: Qt.rgba(1, 1, 1, 0.65)
                                font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Medium
                            }
                        }
                    }
                }
            }
        }

        // Right column: Red month label + 7-column month grid
        Column {
            width: parent.width * 0.48
            height: parent.height
            spacing: 6

            Item {
                width: parent.width; height: 20
                Text {
                    anchors.right: parent.right; anchors.rightMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    text: Qt.formatDate(c.now, "MMM").toUpperCase()
                    color: "#ff453a"
                    font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                }
            }

            Grid {
                width: parent.width
                columns: 7
                rowSpacing: 3
                columnSpacing: 0
                readonly property real cellW: Math.floor(width / 7)

                Repeater {
                    model: ["S", "M", "T", "W", "T", "F", "S"]
                    Text {
                        required property string modelData
                        width: parent.cellW
                        horizontalAlignment: Text.AlignHCenter
                        text: modelData
                        color: Qt.rgba(1, 1, 1, 0.56)
                        font.family: Theme.font; font.pixelSize: 10; font.weight: Font.DemiBold
                    }
                }
                Repeater {
                    model: c.first + c.days
                    Item {
                        required property int index
                        readonly property int day: index - c.first + 1
                        readonly property bool isToday: day === c.now.getDate()
                        width: parent.cellW; height: 22
                        Rectangle {
                            anchors.centerIn: parent
                            width: 22; height: 22; radius: 11
                            color: "#ff453a"
                            visible: parent.isToday
                        }
                        Text {
                            anchors.centerIn: parent
                            text: parent.day > 0 ? parent.day : ""
                            color: parent.isToday ? "white" : (parent.day < c.now.getDate() ? Qt.rgba(1, 1, 1, 0.62) : "white")
                            font.family: Theme.font
                            font.pixelSize: 11
                            font.weight: parent.isToday ? Font.Bold : Font.Medium
                            font.features: { "tnum": 1 }
                        }
                    }
                }
            }
        }
    }
}
