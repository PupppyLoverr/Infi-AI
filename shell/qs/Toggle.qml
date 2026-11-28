import QtQuick
Item {
    id: t
    property string icon; property string title; property string sub; property bool on
    signal clicked
    width: parent ? parent.width : 140; height: 34
    Row {
        spacing: 10
        Rectangle { width: 32; height: 32; radius: 16; color: t.on ? Theme.accent : Theme.fill
            Icon { anchors.centerIn: parent; name: t.icon; size: 16; color: t.on ? "white" : Theme.ink } }
        Column { anchors.verticalCenter: parent.verticalCenter
            Text { text: t.title; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.DemiBold }
            Text { text: t.sub; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 11; width: 100; elide: Text.ElideRight; font.weight: Theme.wBody;} }
    }
    TapHandler { onTapped: t.clicked() }
}
