import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris
import Quickshell.Widgets

// Exact getdroppy.app 3-Column Multi-Widget Dashboard & Widget Customizer:
// - Dashboard mode: up to 3 active widget cards side-by-side (Now Playing,
//   Calendar Today, Blue Sky Hourly Weather, or Pomodoro / Cleaner / Mixer / Usage).
// - Customize mode: red `−` remove badges on active cards + bottom Widget Picker
//   Tray with vibrant blue checkmarks and circular `✕` / `✓` buttons.
Item {
    id: wd
    property var shell
    property MprisPlayer player
    property bool customizing: false
    signal openWidget(string widgetTab)

    property var activeWidgets: ["media", "calendar", "weather"]
    property var draftWidgets: ["media", "calendar", "weather"]
    readonly property var shownWidgets: customizing ? draftWidgets : activeWidgets

    readonly property var catalog: [
        { id: "media",    label: "Now Playing", icon: "music",    tint: "#ff375f", tab: "media" },
        { id: "calendar", label: "Calendar",    icon: "calendar", tint: "#ff453a", tab: "cal" },
        { id: "weather",  label: "Weather",     icon: "sun",      tint: "#38a4ff", tab: "widgets" },
        { id: "pomodoro", label: "Pomodoro",    icon: "timer",    tint: "#ff9f0a", tab: "pomodoro" },
        { id: "cleaner",  label: "Cleaner",     icon: "sparkles", tint: "#3ce8c5", tab: "cleaner" },
        { id: "mixer",    label: "Audio Mixer", icon: "volume",   tint: "#ff9f0a", tab: "mixer" },
        { id: "usage",    label: "AI Usage",    icon: "gauge",    tint: "#30d158", tab: "usage" }
    ]

    // Live wttr.in hourly weather for the Blue Sky Weather Card
    property string wPlace: "San Francisco"
    property string wTemp: "18°"
    property string wCond: "Partly Cloudy"
    property string wRange: "H:21°  L:12°"
    property var wHourly: [
        { hr: "Now", t: "18°", ic: "sun" },
        { hr: "12p", t: "19°", ic: "sun" },
        { hr: "1p",  t: "20°", ic: "sun" },
        { hr: "2p",  t: "21°", ic: "sun" },
        { hr: "3p",  t: "19°", ic: "moon" }
    ]

    Process {
        id: weatherProc
        running: true
        command: ["curl", "-fsS", "--max-time", "7", "https://wttr.in/?format=j1"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const j = JSON.parse(text)
                    const c = j.current_condition[0]
                    wd.wTemp = c.temp_C + "°"
                    wd.wCond = c.weatherDesc[0].value
                    const a = j.nearest_area && j.nearest_area[0]
                    if (a) wd.wPlace = a.areaName[0].value
                    const d = j.weather && j.weather[0]
                    if (d) {
                        wd.wRange = "H:" + d.maxtempC + "°  L:" + d.mintempC + "°"
                        if (d.hourly && d.hourly.length >= 5) {
                            const hs = []
                            for (let i = 0; i < 5; i++) {
                                const h = d.hourly[i]
                                const hrNum = Math.floor((parseInt(h.time) || 0) / 100)
                                const lbl = i === 0 ? "Now" : (hrNum === 0 ? "12a" : (hrNum < 12 ? hrNum + "a" : (hrNum === 12 ? "12p" : (hrNum - 12) + "p")))
                                hs.push({ hr: lbl, t: h.tempC + "°", ic: hrNum >= 18 || hrNum < 6 ? "moon" : "sun" })
                            }
                            wd.wHourly = hs
                        }
                    }
                } catch (e) {}
            }
        }
    }

    FileView {
        id: cfgFile
        path: Theme.home + "/.config/solyra/widgets.json"
        onLoaded: {
            try {
                const a = JSON.parse(text())
                if (Array.isArray(a) && a.length > 0) {
                    wd.activeWidgets = a.slice(0, 3)
                    wd.draftWidgets = a.slice(0, 3)
                }
            } catch (e) {}
        }
    }

    function saveDraft() {
        activeWidgets = draftWidgets.length > 0 ? draftWidgets.slice(0, 3) : ["media", "calendar", "weather"]
        customizing = false
        Quickshell.execDetached([
            "sh", "-c",
            "mkdir -p \"$HOME/.config/solyra\" && printf '%s' \"$1\" > \"$HOME/.config/solyra/widgets.json\"",
            "sh", JSON.stringify(activeWidgets)
        ])
    }

    function cancelDraft() {
        draftWidgets = activeWidgets.slice()
        customizing = false
    }

    function toggleWidgetInDraft(id) {
        const idx = draftWidgets.indexOf(id)
        if (idx >= 0) {
            if (draftWidgets.length > 1) draftWidgets = draftWidgets.filter(x => x !== id)
        } else {
            const next = draftWidgets.concat([id])
            draftWidgets = next.slice(-3)
        }
    }

    function removeFromDraft(id) {
        if (draftWidgets.length > 1) draftWidgets = draftWidgets.filter(x => x !== id)
    }

    Column {
        anchors.fill: parent
        spacing: 10

        // Active 3-column horizontal widget strip
        Row {
            id: cardsRow
            width: parent.width
            height: wd.customizing ? parent.height - 76 : parent.height - 26
            spacing: 10

            Repeater {
                model: wd.shownWidgets
                Item {
                    id: slot
                    required property string modelData
                    readonly property int count: Math.max(1, wd.shownWidgets.length)
                    width: (cardsRow.width - (count - 1) * 10) / count
                    height: cardsRow.height

                    // --- CARD 1: NOW PLAYING ---
                    Rectangle {
                        visible: slot.modelData === "media"
                        anchors.fill: parent
                        radius: 18
                        color: "#131419"
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.10)
                        Column {
                            anchors.fill: parent; anchors.margins: 12; spacing: 10
                            Row {
                                width: parent.width; spacing: 9
                                ClippingRectangle {
                                    width: 40; height: 40; radius: 9; color: "#23232c"
                                    Rectangle {
                                        anchors.fill: parent
                                        gradient: Gradient {
                                            GradientStop { position: 0; color: "#ff7a45" }
                                            GradientStop { position: 1; color: "#5f27cd" }
                                        }
                                    }
                                    Image { anchors.fill: parent; source: wd.player ? wd.player.trackArtUrl : ""; fillMode: Image.PreserveAspectCrop }
                                }
                                Column {
                                    width: parent.width - 50
                                    anchors.verticalCenter: parent.verticalCenter
                                    spacing: 1
                                    Text { width: parent.width; elide: Text.ElideRight; text: wd.player ? (wd.player.trackTitle || "Now Playing") : "Nothing Playing"; color: "white"; font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold }
                                    Text { width: parent.width; elide: Text.ElideRight; text: wd.player ? (wd.player.trackArtist || wd.player.identity) : "Open music"; color: Qt.rgba(1, 1, 1, 0.56); font.family: Theme.font; font.pixelSize: 11; font.weight: Theme.wBody;}
                                }
                            }
                            Rectangle {
                                width: parent.width; height: 4; radius: 2; color: Qt.rgba(1, 1, 1, 0.18)
                                Rectangle {
                                    height: parent.height; radius: 2; color: "white"
                                    width: wd.player && wd.player.length > 0 ? parent.width * Math.min(1, wd.player.position / wd.player.length) : parent.width * 0.35
                                }
                            }
                            Row {
                                anchors.horizontalCenter: parent.horizontalCenter
                                spacing: 22
                                Icon { name: "prev"; size: 17; color: "white"; TapHandler { onTapped: if (wd.player && wd.player.canGoPrevious) wd.player.previous() } }
                                Icon { name: wd.player && wd.player.isPlaying ? "pause" : "play"; size: 22; color: "white"; TapHandler { onTapped: if (wd.player) wd.player.togglePlaying() } }
                                Icon { name: "next"; size: 17; color: "white"; TapHandler { onTapped: if (wd.player && wd.player.canGoNext) wd.player.next() } }
                            }
                        }
                        TapHandler { onTapped: if (!wd.customizing) wd.openWidget("media") }
                    }

                    // --- CARD 2: CALENDAR TODAY ---
                    Rectangle {
                        visible: slot.modelData === "calendar"
                        anchors.fill: parent
                        radius: 18
                        color: "#131419"
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.10)
                        Column {
                            anchors.fill: parent; anchors.margins: 12; spacing: 8
                            Row {
                                spacing: 5
                                Text { text: Qt.formatDate(new Date(), "ddd").toUpperCase(); color: "#ff453a"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold }
                                Text { text: new Date().getDate(); color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.Bold }
                            }
                            Repeater {
                                model: wd.shell ? wd.shell.calendarEvents.slice(0, 2) : []
                                Rectangle {
                                    required property var modelData
                                    width: parent.width; height: 40; radius: 9
                                    color: modelData.bg || "#201c3a"
                                    Rectangle { x: 6; y: 6; width: 3; height: parent.height - 12; radius: 1.5; color: modelData.accent || "#8e82ff" }
                                    Column {
                                        anchors { left: parent.left; leftMargin: 15; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
                                        spacing: 1
                                        Text { width: parent.width; elide: Text.ElideRight; text: modelData.title; color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold }
                                        Text { width: parent.width; elide: Text.ElideRight; text: modelData.time; color: Qt.rgba(1, 1, 1, 0.60); font.family: Theme.font; font.pixelSize: 10; font.weight: Theme.wBody;}
                                    }
                                }
                            }
                        }
                        TapHandler { onTapped: if (!wd.customizing) wd.openWidget("cal") }
                    }

                    // --- CARD 3: BLUE SKY HOURLY WEATHER (exact Droppy match!) ---
                    Rectangle {
                        visible: slot.modelData === "weather"
                        anchors.fill: parent
                        radius: 18
                        gradient: Gradient {
                            GradientStop { position: 0.0; color: "#1d63c6" }
                            GradientStop { position: 0.55; color: "#3284e8" }
                            GradientStop { position: 1.0; color: "#58a6f7" }
                        }
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.22)
                        Column {
                            anchors.fill: parent; anchors.margins: 12; spacing: 6
                            Item {
                                width: parent.width; height: 42
                                Column {
                                    anchors.left: parent.left; spacing: 0
                                    Text { text: wd.wPlace; color: "white"; font.family: Theme.font; font.pixelSize: 12; font.weight: Font.DemiBold }
                                    Text { text: wd.wTemp; color: "white"; font.family: Theme.font; font.pixelSize: 28; font.weight: Font.Light }
                                }
                                Column {
                                    anchors.right: parent.right; anchors.top: parent.top; spacing: 2
                                    Text { anchors.right: parent.right; text: wd.wCond; color: Qt.rgba(1, 1, 1, 0.90); font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Medium }
                                    Text { anchors.right: parent.right; text: wd.wRange; color: Qt.rgba(1, 1, 1, 0.78); font.family: Theme.font; font.pixelSize: 10; font.weight: Theme.wBody;}
                                }
                            }
                            Row {
                                width: parent.width
                                anchors.horizontalCenter: parent.horizontalCenter
                                Repeater {
                                    model: wd.wHourly
                                    Column {
                                        required property var modelData
                                        width: parent.width / 5
                                        spacing: 4
                                        Text { anchors.horizontalCenter: parent.horizontalCenter; text: modelData.hr; color: Qt.rgba(1, 1, 1, 0.80); font.family: Theme.font; font.pixelSize: 9; font.weight: Font.DemiBold }
                                        Icon { anchors.horizontalCenter: parent.horizontalCenter; name: modelData.ic; size: 13; color: modelData.ic === "sun" ? "#ffd60a" : "white" }
                                        Text { anchors.horizontalCenter: parent.horizontalCenter; text: modelData.t; color: "white"; font.family: Theme.font; font.pixelSize: 10; font.weight: Font.DemiBold }
                                    }
                                }
                            }
                        }
                    }

                    // --- MINI CARDS FOR POMODORO / CLEANER / MIXER / USAGE ---
                    Rectangle {
                        visible: slot.modelData === "pomodoro" || slot.modelData === "cleaner" || slot.modelData === "mixer" || slot.modelData === "usage"
                        anchors.fill: parent
                        radius: 18
                        color: "#131419"
                        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.10)
                        Column {
                            anchors.centerIn: parent
                            spacing: 8
                            Rectangle {
                                anchors.horizontalCenter: parent.horizontalCenter
                                width: 42; height: 42; radius: 21
                                color: slot.modelData === "pomodoro" ? "#452d0a"
                                     : slot.modelData === "cleaner" ? "#1a4b44"
                                     : slot.modelData === "mixer" ? "#4a2f08"
                                     : "#113d27"
                                Icon {
                                    anchors.centerIn: parent; size: 20
                                    name: slot.modelData === "pomodoro" ? "timer"
                                        : slot.modelData === "cleaner" ? "sparkles"
                                        : slot.modelData === "mixer" ? "volume"
                                        : "gauge"
                                    color: slot.modelData === "pomodoro" ? "#ff9f0a"
                                         : slot.modelData === "cleaner" ? "#3ce8c5"
                                         : slot.modelData === "mixer" ? "#ff9f0a"
                                         : "#30d158"
                                }
                            }
                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                text: slot.modelData === "pomodoro" ? "Pomodoro · 25:00"
                                    : slot.modelData === "cleaner" ? "Storage Cleaner"
                                    : slot.modelData === "mixer" ? "Audio Mixer"
                                    : "AI Usage"
                                color: "white"
                                font.family: Theme.font; font.pixelSize: 13; font.weight: Font.Bold
                            }
                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                text: "Click to open"
                                color: Qt.rgba(1, 1, 1, 0.48)
                                font.family: Theme.font; font.pixelSize: 11
                                font.weight: Theme.wBody
                            }
                        }
                        TapHandler { onTapped: if (!wd.customizing) wd.openWidget(slot.modelData) }
                    }

                    // Droppy Widget Customizer red `−` top-left remove circle badge!
                    Rectangle {
                        visible: wd.customizing
                        x: -5; y: -5
                        width: 22; height: 22; radius: 11
                        color: "#ff453a"
                        border.width: 2; border.color: "#000000"
                        Text { anchors.centerIn: parent; text: "−"; color: "white"; font.pixelSize: 15; font.weight: Font.Bold }
                        TapHandler { onTapped: wd.removeFromDraft(slot.modelData) }
                    }
                }
            }
        }

        // Normal footer quick-launch bar (switches straight into Pomodoro / Cleaner / Mixer / Usage / Customize)
        Item {
            visible: !wd.customizing
            width: parent.width
            height: 22
            Row {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                Repeater {
                    model: [
                        { id: "pomodoro", label: "Pomodoro", icon: "timer",    tint: "#ff9f0a" },
                        { id: "cleaner",  label: "Cleaner",  icon: "sparkles", tint: "#3ce8c5" },
                        { id: "mixer",    label: "Mixer",    icon: "volume",   tint: "#ff9f0a" },
                        { id: "usage",    label: "Usage",    icon: "gauge",    tint: "#30d158" }
                    ]
                    Rectangle {
                        required property var modelData
                        height: 22; width: wPillRow.implicitWidth + 16; radius: 11
                        color: Qt.rgba(1, 1, 1, 0.09)
                        Row {
                            id: wPillRow
                            anchors.centerIn: parent; spacing: 5
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: modelData.icon; size: 11; color: modelData.tint }
                            Text { anchors.verticalCenter: parent.verticalCenter; text: modelData.label; color: Qt.rgba(1, 1, 1, 0.85); font.family: Theme.font; font.pixelSize: 11; font.weight: Font.Medium }
                        }
                        TapHandler { onTapped: wd.openWidget(modelData.id) }
                    }
                }
            }
            Rectangle {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                height: 22; width: custRow.implicitWidth + 16; radius: 11
                color: Qt.rgba(1, 1, 1, 0.12)
                Row {
                    id: custRow; anchors.centerIn: parent; spacing: 5
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "sliders"; size: 11; color: "white" }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: "Customize"; color: "white"; font.family: Theme.font; font.pixelSize: 11; font.weight: Font.DemiBold }
                }
                TapHandler { onTapped: { wd.draftWidgets = wd.activeWidgets.slice(); wd.customizing = true } }
            }
        }

        // Droppy Widget Picker Tray (shown in Customize mode: blue checkmark tiles + ✕ / ✓)
        Rectangle {
            visible: wd.customizing
            width: parent.width
            height: 64
            radius: 16
            color: "#111216"
            border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.12)

            Row {
                anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
                spacing: 8
                Repeater {
                    model: wd.catalog
                    Item {
                        required property var modelData
                        readonly property bool chosen: wd.draftWidgets.indexOf(modelData.id) >= 0
                        width: 74; height: 48
                        Rectangle {
                            anchors.fill: parent; radius: 11
                            color: chosen ? Qt.rgba(0.04, 0.52, 1.0, 0.20) : Qt.rgba(1, 1, 1, 0.06)
                            border.width: 1
                            border.color: chosen ? "#0a84ff" : Qt.rgba(1, 1, 1, 0.08)
                            Column {
                                anchors.centerIn: parent; spacing: 4
                                Icon { anchors.horizontalCenter: parent.horizontalCenter; name: modelData.icon; size: 16; color: modelData.tint }
                                Text { anchors.horizontalCenter: parent.horizontalCenter; text: modelData.label; color: "white"; font.family: Theme.font; font.pixelSize: 10; font.weight: Font.Medium }
                            }
                        }
                        // Blue circular checkmark badge when active
                        Rectangle {
                            visible: chosen
                            anchors { top: parent.top; right: parent.right; margins: -3 }
                            width: 16; height: 16; radius: 8
                            color: "#0a84ff"
                            Icon { anchors.centerIn: parent; name: "check"; size: 10; color: "white" }
                        }
                        TapHandler { onTapped: wd.toggleWidgetInDraft(modelData.id) }
                    }
                }
            }

            Row {
                anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
                spacing: 6
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: Qt.rgba(1, 1, 1, 0.12)
                    Icon { anchors.centerIn: parent; name: "x"; size: 13; color: "white" }
                    TapHandler { onTapped: wd.cancelDraft() }
                }
                Rectangle {
                    width: 30; height: 30; radius: 15
                    color: "#0a84ff"
                    Icon { anchors.centerIn: parent; name: "check"; size: 14; color: "white" }
                    TapHandler { onTapped: wd.saveDraft() }
                }
            }
        }
    }
}
