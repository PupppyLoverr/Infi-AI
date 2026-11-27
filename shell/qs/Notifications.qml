import QtQuick
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Notifications

// The notification server and its banners: glass cards top-right that slide
// in from the edge, pause their timeout while the pointer is on them (a close
// button shows then), swipe right to dismiss, and stay quiet during Focus
// (Do Not Disturb) unless critical. Every notification also goes to the
// Notification Center history (shell.history), which the clock opens.
PanelWindow {
    id: n
    property var shell
    anchors { top: true; right: true }
    margins { top: 36; right: 10 }
    implicitWidth: 360
    implicitHeight: Math.max(1, stack.height)
    exclusiveZone: -1
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "solyra-notifications"
    visible: popups.length > 0 && !(shell && shell.centerOpen)

    readonly property int maxBanners: 4
    // Live notification objects (a ListModel cannot hold QObjects).
    property var popups: []
    function drop(note) { popups = popups.filter(x => x !== note) }
    NotificationServer {
        keepOnReload: false
        bodySupported: true; actionsSupported: true; imageSupported: true; persistenceSupported: true
        onNotification: note => {
            note.tracked = true
            if (n.shell) n.shell.remember(note)
            note.closed.connect(() => { n.drop(note); if (n.shell) n.shell.forget(note) })
            if (n.shell && n.shell.dnd && note.urgency !== NotificationUrgency.Critical) return
            n.popups = [note].concat(n.popups).slice(0, n.maxBanners)
            // Forward messaging / replyable notifications into the Dynamic Island for inline quick-reply
            if (n.shell && /^(messages|signal|telegram|discord|slack|element|whatsapp|thunderbird|mail)/i.test(note.appName || "")) {
                n.shell.triggerReplyBanner(note.summary || note.appName, note.body || "", note.appName || "Messages")
            }
        }
    }
    Column {
        id: stack
        width: parent.width
        spacing: 8
        move: Transition { NumberAnimation { properties: "y"; duration: Theme.ms(220); easing.type: Easing.OutCubic } }
        Repeater {
            model: n.popups
            Item {
                id: banner
                required property var modelData
                readonly property var note: modelData
                width: stack.width; height: card.height
                // Slide in from the right edge.
                property real slide: Theme.reduceMotion ? 0 : stack.width
                Component.onCompleted: slide = 0
                Behavior on slide { NumberAnimation { duration: Theme.ms(320); easing.type: Easing.OutBack; easing.overshoot: 0.8 } }

                Glass {
                    id: card
                    x: banner.slide + drag.offset
                    opacity: 1 - Math.max(0, drag.offset) / (stack.width * 0.9)
                    width: parent.width; height: col.height + 24; r: 20
                    HoverHandler { id: hov }
                    Timer {
                        interval: banner.note.urgency === NotificationUrgency.Critical ? 0 : (banner.note.expireTimeout > 0 ? banner.note.expireTimeout : 6000)
                        running: interval > 0 && !hov.hovered && !drag.active
                        onTriggered: n.drop(banner.note)
                    }
                    Row {
                        x: 14; y: 12; width: parent.width - 28; spacing: 10
                        Image {
                            id: pic
                            visible: status === Image.Ready
                            width: visible ? 40 : 0; height: 40
                            source: banner.note.image || ""
                            fillMode: Image.PreserveAspectCrop
                            sourceSize: Qt.size(80, 80)
                        }
                        Column {
                            id: col
                            width: parent.width - (pic.visible ? 50 : 0); spacing: 3
                            Row { spacing: 8; width: parent.width
                                Image { width: 16; height: 16; source: banner.note.appIcon ? Quickshell.iconPath(banner.note.appIcon, true) : ""; visible: source != "" }
                                Text { text: banner.note.appName || "Notification"; color: Theme.ink2; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold }
                                Text { text: "now"; color: Theme.ink3; font.family: Theme.font; font.pixelSize: 11 }
                            }
                            Text { width: parent.width; text: banner.note.summary; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 14; font.weight: Font.DemiBold }
                            Text { width: parent.width; visible: text !== ""; text: banner.note.body; wrapMode: Text.Wrap; maximumLineCount: 4; elide: Text.ElideRight; color: Theme.ink; font.family: Theme.font; font.pixelSize: 13; textFormat: Text.PlainText }
                            Row { spacing: 6; visible: banner.note.actions.length > 0; topPadding: 4
                                Repeater { model: banner.note.actions; Pill { required property var modelData; height: 26; text: modelData.text; onClicked: { modelData.invoke(); n.drop(banner.note) } } } }
                        }
                    }
                    // Close, shown on hover (top-left, as macOS).
                    Rectangle {
                        visible: hov.hovered
                        x: -6; y: -6; width: 20; height: 20; radius: 10
                        color: Theme.dark ? "#3a3a3e" : "#e8e8ec"
                        border.width: 1; border.color: Theme.rim
                        Icon { anchors.centerIn: parent; name: "x"; size: 10; color: Theme.ink }
                        TapHandler { onTapped: banner.note.dismiss() }
                    }
                    // Click: the default action if any, then gone from screen
                    // (it stays in the Notification Center).
                    TapHandler {
                        onTapped: {
                            const def = banner.note.actions.find(a => a.identifier === "default")
                            if (def) def.invoke()
                            n.drop(banner.note)
                        }
                    }
                }
                // Swipe right to dismiss.
                DragHandler {
                    id: drag
                    target: null
                    xAxis.enabled: true; yAxis.enabled: false
                    property real offset: 0
                    onActiveTranslationChanged: offset = Math.max(0, activeTranslation.x)
                    onActiveChanged: if (!active) { if (offset > stack.width * 0.35) banner.note.dismiss(); offset = 0 }
                }
            }
        }
    }
}
