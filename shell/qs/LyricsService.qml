import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Mpris

// Shared live synced lyrics service for both the Dynamic Island Media drawer
// and the Droppy Fullscreen Ambient Lock Screen.
//
// Lookup order (never invents fake lyrics):
//   1. Local .lrc in ~/.local/share/solyra/lyrics/ ("Artist - Title.lrc" or "Title.lrc")
//   2. Cached .lrc in ~/.cache/solyra/lyrics/<slug>.lrc
//   3. Live LRCLIB API (/api/get, falling back to /api/search), preferring
//      syncedLyrics and falling back to evenly spaced plainLyrics; caches to disk.
QtObject {
    id: svc

    readonly property MprisPlayer player: {
        const ps = Mpris.players.values
        let best = null
        for (let i = 0; i < ps.length; i++) {
            const p = ps[i]
            if (p.isPlaying) return p
            if (!best) best = p
        }
        return best
    }

    property var lines: []
    property bool loading: false
    property string source: ""
    property string currentKey: ""
    property string pendingSlug: ""

    function slug(artist, title) {
        return ((artist || "") + "-" + (title || ""))
            .toLowerCase()
            .replace(/[^a-z0-9]+/g, "_")
            .replace(/^_+|_+$/g, "")
            .slice(0, 96) || "track"
    }

    function parseLrc(raw) {
        const out = []
        const rawLines = (raw || "").split("\n")
        for (let i = 0; i < rawLines.length; i++) {
            const line = rawLines[i].trim()
            if (!line) continue
            const stamps = []
            let rest = line
            while (rest.startsWith("[")) {
                const close = rest.indexOf("]")
                if (close < 0) break
                const tag = rest.slice(1, close)
                const m = tag.match(/^(\d+):(\d+(?:\.\d+)?)$/)
                if (m) stamps.push(parseInt(m[1], 10) * 60 + parseFloat(m[2]))
                rest = rest.slice(close + 1)
            }
            const text = rest.trim()
            if (!text || stamps.length === 0) continue
            for (let j = 0; j < stamps.length; j++) out.push({ t: stamps[j], line: text })
        }
        out.sort((a, b) => a.t - b.t)
        return out
    }

    function parsePlain(raw, durSec) {
        const texts = (raw || "").split("\n").map(s => s.trim()).filter(s => s.length > 0)
        if (texts.length === 0) return []
        const dur = Math.max(60, durSec > 0 ? durSec : 180)
        const step = dur / texts.length
        return texts.map((line, i) => ({ t: i * step, line: line }))
    }

    function toLrcText(arr) {
        let out = ""
        for (let i = 0; i < arr.length; i++) {
            const sec = Math.max(0, arr[i].t)
            const m = Math.floor(sec / 60)
            const s = Math.floor(sec % 60)
            const cs = Math.floor((sec - Math.floor(sec)) * 100)
            out += "[" + String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0") + "." + String(cs).padStart(2, "0") + "] " + arr[i].line + "\n"
        }
        return out
    }

    function indexAt(posSec) {
        if (!lines || lines.length === 0) return -1
        let idx = 0
        for (let i = 0; i < lines.length; i++) {
            if (lines[i].t <= posSec) idx = i
            else break
        }
        return idx
    }

    function refresh() {
        const p = svc.player
        if (!p || !p.trackTitle) {
            currentKey = ""
            lines = []
            source = ""
            loading = false
            return
        }
        const artist = p.trackArtist || ""
        const title = p.trackTitle || ""
        const key = artist + "|" + title
        if (key === currentKey) return
        currentKey = key
        lines = []
        source = ""
        loading = true
        pendingSlug = slug(artist, title)
        fetchProc.running = false
        fetchProc.command = [
            "sh", "-c",
            "artist=\"$1\"; title=\"$2\"; sl=\"$3\"; " +
            "data=\"${XDG_DATA_HOME:-$HOME/.local/share}/solyra/lyrics\"; " +
            "cache=\"${XDG_CACHE_HOME:-$HOME/.cache}/solyra/lyrics/$sl.lrc\"; " +
            "for f in \"$data/$artist - $title.lrc\" \"$data/$title.lrc\"; do " +
            "  if [ -f \"$f\" ] && [ -s \"$f\" ]; then printf 'LOCAL\\n'; cat \"$f\"; exit 0; fi; " +
            "done; " +
            "if [ -f \"$cache\" ] && [ -s \"$cache\" ]; then printf 'CACHE\\n'; cat \"$cache\"; exit 0; fi; " +
            "res=$(curl -fsSL --max-time 5 -G -H 'User-Agent: SolyraOS/0.1 (https://github.com/PupppyLoverr/Infi-AI)' " +
            "  'https://lrclib.net/api/get' --data-urlencode \"artist_name=$artist\" --data-urlencode \"track_name=$title\" 2>/dev/null || true); " +
            "if [ -n \"$res\" ] && printf '%s' \"$res\" | grep -qE 'syncedLyrics|plainLyrics'; then " +
            "  printf 'JSON\\n%s' \"$res\"; exit 0; " +
            "fi; " +
            "q=\"$artist $title\"; " +
            "res=$(curl -fsSL --max-time 5 -G -H 'User-Agent: SolyraOS/0.1 (https://github.com/PupppyLoverr/Infi-AI)' " +
            "  'https://lrclib.net/api/search' --data-urlencode \"q=$q\" 2>/dev/null || true); " +
            "if [ -n \"$res\" ]; then printf 'JSON\\n%s' \"$res\"; fi",
            "sh", artist, title, pendingSlug
        ]
        fetchProc.running = true
    }

    onPlayerChanged: refresh()

    property var playerConn: Connections {
        target: svc.player
        function onTrackTitleChanged() { svc.refresh() }
        function onTrackArtistChanged() { svc.refresh() }
    }

    property var fetchProc: Process {
        stdout: StdioCollector {
            onStreamFinished: {
                svc.loading = false
                const raw = text || ""
                const nl = raw.indexOf("\n")
                if (nl < 0) { svc.lines = []; svc.source = ""; return }
                const tag = raw.slice(0, nl).trim()
                const body = raw.slice(nl + 1)
                if (tag === "LOCAL" || tag === "CACHE") {
                    svc.lines = svc.parseLrc(body)
                    svc.source = tag.toLowerCase()
                    return
                }
                if (tag === "JSON") {
                    try {
                        let obj = JSON.parse(body)
                        if (Array.isArray(obj)) {
                            let best = null
                            for (let i = 0; i < obj.length; i++) {
                                if (obj[i] && obj[i].syncedLyrics) { best = obj[i]; break }
                                if (!best && obj[i] && obj[i].plainLyrics) best = obj[i]
                            }
                            obj = best
                        }
                        if (!obj) { svc.lines = []; svc.source = ""; return }
                        let parsed = []
                        let src = ""
                        if (obj.syncedLyrics) {
                            parsed = svc.parseLrc(obj.syncedLyrics)
                            src = "lrclib"
                        }
                        if (parsed.length === 0 && obj.plainLyrics) {
                            const dur = svc.player && svc.player.length > 0 ? svc.player.length : 180
                            parsed = svc.parsePlain(obj.plainLyrics, dur)
                            src = "plain"
                        }
                        svc.lines = parsed
                        svc.source = src
                        if (parsed.length > 0 && svc.pendingSlug) {
                            const lrc = svc.toLrcText(parsed)
                            Quickshell.execDetached([
                                "sh", "-c",
                                "dir=\"${XDG_CACHE_HOME:-$HOME/.cache}/solyra/lyrics\"; mkdir -p \"$dir\"; printf '%s' \"$2\" > \"$dir/$1.lrc\"",
                                "sh", svc.pendingSlug, lrc
                            ])
                        }
                    } catch (e) {
                        svc.lines = []
                        svc.source = ""
                    }
                }
            }
        }
    }
}
