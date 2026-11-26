import QtQuick

// Calendar glance: the month with today circled, and the date.
Item {
    id: c
    readonly property date now: new Date()
    readonly property int first: new Date(now.getFullYear(), now.getMonth(), 1).getDay()
    readonly property int days: new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate()
    Row {
        anchors.fill: parent
        spacing: 22
        Column {
            width: 150; spacing: 2
            Text { text: Qt.formatDate(c.now, "dddd"); color: "#ff453a"; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold }
            Text { text: c.now.getDate(); color: "white"; font.family: Theme.font; font.pixelSize: 54; font.weight: Font.Bold }
            Text { text: Qt.formatDate(c.now, "MMMM yyyy"); color: Qt.rgba(1, 1, 1, 0.6); font.family: Theme.font; font.pixelSize: 13 }
        }
        Grid {
            columns: 7; rowSpacing: 2; columnSpacing: 2
            Repeater {
                model: ["S", "M", "T", "W", "T", "F", "S"]
                Text { required property string modelData; width: 38; horizontalAlignment: Text.AlignHCenter; text: modelData; color: Qt.rgba(1, 1, 1, 0.45); font.family: Theme.font; font.pixelSize: 10; font.weight: Font.DemiBold }
            }
            Repeater {
                model: c.first + c.days
                Item {
                    required property int index
                    readonly property int day: index - c.first + 1
                    width: 38; height: 20
                    Rectangle { anchors.centerIn: parent; width: 20; height: 20; radius: 10; color: "#ff453a"; visible: parent.day === c.now.getDate() }
                    Text { anchors.centerIn: parent; text: parent.day > 0 ? parent.day : ""; color: "white"; font.family: Theme.font; font.pixelSize: 11 }
                }
            }
        }
    }
}
