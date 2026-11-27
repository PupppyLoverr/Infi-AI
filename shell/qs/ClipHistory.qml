import QtQuick
import Quickshell
import Quickshell.Io

// Persistent Droppy-style clipboard history backed by wl-paste --watch and
// ~/.local/share/solyra/clipboard.json. Skips password-manager secrets,
// classifies Color (#RGB / #RRGGBB), Link, Video, and Text, and supports
// Work/Personal categories, pinning, favorites, and Incognito mode.
ListModel {
    id: h
    property bool incognito: false
    property int rev: 0
    readonly property string storePath: Theme.home + "/.local/share/solyra/clipboard.json"

    function classify(raw) {
        const t = (raw || "").trim()
        if (/^#([0-9a-f]{6}|[0-9a-f]{3})$/i.test(t)) return "color"
        if (/^https?:\/\//i.test(t)) return "link"
        if (/\.(mp4|mov|mkv|webm)$/i.test(t)) return "video"
        return "text"
    }

    function normalizeHex(raw) {
        const t = (raw || "").trim()
        if (/^#[0-9a-f]{6}$/i.test(t)) return t.toUpperCase()
        if (/^#[0-9a-f]{3}$/i.test(t)) {
            return ("#" + t[1] + t[1] + t[2] + t[2] + t[3] + t[3]).toUpperCase()
        }
        return "#0A84FF"
    }

    function parseRgb(raw) {
        const hex = normalizeHex(raw)
        return [
            parseInt(hex.slice(1, 3), 16),
            parseInt(hex.slice(3, 5), 16),
            parseInt(hex.slice(5, 7), 16)
        ]
    }

    function rgbLabel(raw) {
        const c = parseRgb(raw)
        return "RGB " + c[0] + ", " + c[1] + ", " + c[2]
    }

    function isBrightHex(raw) {
        const c = parseRgb(raw)
        return (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) > 155
    }

    function ageLabel(ts) {
        if (!ts || ts <= 0) return "now"
        const s = Math.max(0, Math.floor((Date.now() - ts) / 1000))
        if (s < 45) return "now"
        if (s < 3600) return Math.floor(s / 60) + " min. ago"
        if (s < 86400) return Math.floor(s / 3600) + " hr ago"
        return Math.floor(s / 86400) + "d ago"
    }

    function save() {
        const arr = []
        for (let i = 0; i < h.count && i < 40; i++) {
            const it = h.get(i)
            arr.push({
                text: it.text,
                kind: it.kind,
                pinned: !!it.pinned,
                favorite: !!it.favorite,
                category: it.category || "all",
                ts: it.ts || Date.now()
            })
        }
        const json = JSON.stringify(arr, null, 2)
        Quickshell.execDetached([
            "sh", "-c",
            "mkdir -p \"$(dirname \"$1\")\" && printf '%s' \"$2\" > \"$1\"",
            "sh", h.storePath, json
        ])
        h.rev += 1
    }

    function pushText(t, cat) {
        if (!t || !t.trim() || t.length > 8192) return
        for (let i = 0; i < h.count; i++) {
            if (h.get(i).text === t) {
                h.setProperty(i, "ts", Date.now())
                if (i > 0) h.move(i, 0, 1)
                save()
                return
            }
        }
        const kind = classify(t)
        const autoCat = cat || (kind === "color" ? "work" : kind === "link" ? "personal" : "all")
        h.insert(0, {
            text: t,
            kind: kind,
            pinned: false,
            favorite: false,
            category: autoCat,
            ts: Date.now()
        })
        while (h.count > 40) {
            let k = h.count - 1
            while (k >= 0 && h.get(k).pinned) k--
            if (k < 0) break
            h.remove(k)
        }
        save()
    }

    function findIndexByText(t) {
        for (let i = 0; i < h.count; i++) if (h.get(i).text === t) return i
        return -1
    }

    function togglePin(t) {
        const i = findIndexByText(t)
        if (i < 0) return
        const next = !h.get(i).pinned
        h.setProperty(i, "pinned", next)
        if (next && i > 0) h.move(i, 0, 1)
        save()
    }

    function toggleFav(t) {
        const i = findIndexByText(t)
        if (i < 0) return
        h.setProperty(i, "favorite", !h.get(i).favorite)
        save()
    }

    function setCategory(t, cat) {
        const i = findIndexByText(t)
        if (i < 0) return
        h.setProperty(i, "category", cat)
        save()
    }

    function removeItem(t) {
        const i = findIndexByText(t)
        if (i < 0) return
        h.remove(i)
        save()
    }

    function copy(t) {
        Quickshell.execDetached(["wl-copy", t])
    }

    property var storeFile: FileView {
        path: h.storePath
        onLoaded: {
            if (h.count > 0) return
            try {
                const arr = JSON.parse(text())
                if (!Array.isArray(arr)) return
                for (let i = 0; i < arr.length && i < 40; i++) {
                    const v = arr[i]
                    if (!v || typeof v.text !== "string" || !v.text.trim()) continue
                    h.append({
                        text: v.text,
                        kind: h.classify(v.text),
                        pinned: !!v.pinned,
                        favorite: !!v.favorite,
                        category: v.category || "all",
                        ts: v.ts || Date.now()
                    })
                }
                h.rev += 1
            } catch (e) {}
        }
    }

    property var proc: Process {
        running: true
        command: [
            "wl-paste", "--type", "text", "--watch", "sh", "-c",
            "wl-paste --list-types | grep -qE 'x-kde-passwordManagerHint|application/x-password|x-password-manager' && exit 0; base64 -w0; echo"
        ]
        stdout: SplitParser {
            onRead: line => {
                if (h.incognito || !line) return
                let t = ""
                try { t = decodeURIComponent(escape(Qt.atob(line))) } catch (e) { t = Qt.atob(line) }
                h.pushText(t, "")
            }
        }
    }
}
