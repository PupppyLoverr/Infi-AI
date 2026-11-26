import QtQuick
import Quickshell
import Quickshell.Io

// Clipboard history from wl-paste's watch mode. Secrets (password-manager
// MIME hints) are skipped; Incognito pauses recording; pins stay on top.
ListModel {
    id: h
    property bool incognito: false
    property var proc: Process {
        running: true
        command: ["wl-paste", "--type", "text", "--watch", "sh", "-c",
                  "wl-paste --list-types | grep -q 'x-kde-passwordManagerHint' && exit 0; base64 -w0; echo"]
        stdout: SplitParser {
            onRead: line => {
                if (h.incognito || !line) return
                let t = ""
                try { t = decodeURIComponent(escape(Qt.atob(line))) } catch (e) { t = Qt.atob(line) }
                if (!t.trim() || t.length > 8192) return
                for (let i = 0; i < h.count; i++) if (h.get(i).text === t) { h.move(i, 0, 1); return }
                h.insert(0, { text: t, pinned: false, kind: /^#[0-9a-f]{6}$/i.test(t.trim()) ? "color" : /^https?:\/\//.test(t) ? "link" : "text" })
                while (h.count > 50) { let k = h.count - 1; while (k >= 0 && h.get(k).pinned) k--; if (k < 0) break; h.remove(k) }
            }
        }
    }
    function copy(t) { Quickshell.execDetached(["wl-copy", t]) }
}
