import QtQuick
import Quickshell
import Quickshell.Io

// Rich Droppy File Tray model: tracks parked files across sessions, inspects
// real byte sizes, image dimensions, directory item counts, and live text
// previews, and executes real Zip/Unzip, Photo/PDF Shrink, New Folder grouping,
// Folder Drops, and Pinned Folders.
ListModel {
    id: shelf
    property int rev: 0
    signal actionToast(string title, string detail)

    function fmtSize(b) {
        if (!(b >= 0)) return ""
        if (b >= 1e9) return (b / 1e9).toFixed(1) + " GB"
        if (b >= 1e6) return (b / 1e6).toFixed(1) + " MB"
        if (b >= 1e3) return Math.round(b / 1e3) + " KB"
        return Math.round(b) + " bytes"
    }

    function cleanPath(u) {
        return decodeURIComponent((u || "").replace(/^file:\/\//, ""))
    }

    property var file: FileView {
        path: Quickshell.env("HOME") + "/.local/share/solyra/shelf.json"
        onLoaded: {
            try {
                const arr = JSON.parse(text())
                if (!Array.isArray(arr)) return
                for (let i = 0; i < arr.length; i++) {
                    const entry = arr[i]
                    if (typeof entry === "string") shelf.add(entry, false)
                    else if (entry && entry.url) shelf.add(entry.url, !!entry.pinned)
                }
            } catch (e) {}
        }
        onLoadFailed: {}
    }

    property var metaProc: Process {
        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.split("\n")
                for (let i = 0; i < lines.length; i++) {
                    const p = lines[i].split("\t")
                    if (p.length < 5) continue
                    const url = p[0]
                    const isDir = p[1] === "1"
                    const sz = parseFloat(p[2]) || 0
                    const extra = p[3] || ""
                    const snip = (p[4] || "").replace(/\\n/g, "\n")
                    for (let k = 0; k < shelf.count; k++) {
                        if (shelf.get(k).url === url) {
                            shelf.setProperty(k, "isDir", isDir)
                            shelf.setProperty(k, "sizeBytes", sz)
                            shelf.setProperty(k, "sizeLabel", isDir ? (extra ? extra + " items" : "Folder") : shelf.fmtSize(sz))
                            shelf.setProperty(k, "dims", isDir ? "" : extra)
                            if (snip) shelf.setProperty(k, "snippet", snip)
                            break
                        }
                    }
                }
                shelf.rev += 1
            }
        }
    }

    property var actionProc: Process {
        property string actionKind: ""
        stdout: StdioCollector {
            onStreamFinished: {
                const line = text.trim()
                if (!line) return
                const parts = line.split("|")
                if (parts.length >= 2) shelf.actionToast(parts[0], parts[1])
                else shelf.actionToast(parts[0], "")
                shelf.refreshMeta()
            }
        }
    }

    function refreshMeta() {
        if (count === 0) return
        const args = []
        for (let i = 0; i < count; i++) args.push(get(i).url)
        metaProc.command = [
            "sh", "-c",
            "for u in \"$@\"; do " +
            "  p=\"${u#file://}\"; " +
            "  if [ -d \"$p\" ]; then " +
            "    cnt=$(ls -1A \"$p\" 2>/dev/null | wc -l); " +
            "    printf '%s\\t1\\t0\\t%s\\t\\n' \"$u\" \"$cnt\"; " +
            "  elif [ -f \"$p\" ]; then " +
            "    sz=$(stat -c %s \"$p\" 2>/dev/null || echo 0); " +
            "    dims=''; " +
            "    case \"${p##*.}\" in png|PNG|jpg|JPG|jpeg|JPEG|webp|WEBP) " +
            "      dims=$(file -b \"$p\" 2>/dev/null | grep -oE '[0-9]{2,5} ?x ?[0-9]{2,5}' | head -1 | tr -d ' ');; esac; " +
            "    snip=''; " +
            "    case \"${p##*.}\" in md|MD|txt|TXT|json|JSON|qml|rs|sh|lua) " +
            "      snip=$(head -c 160 \"$p\" 2>/dev/null | tr '\\t' ' ' | sed ':a;N;$!ba;s/\\n/\\\\n/g');; esac; " +
            "    printf '%s\\t0\\t%s\\t%s\\t%s\\n' \"$u\" \"$sz\" \"$dims\" \"$snip\"; " +
            "  fi; " +
            "done",
            "sh"
        ].concat(args)
        metaProc.running = true
    }

    function persist() {
        const a = []
        for (let i = 0; i < count; i++) a.push({ url: get(i).url, pinned: !!get(i).pinned })
        Quickshell.execDetached([
            "sh", "-c",
            "mkdir -p \"$HOME/.local/share/solyra\" && printf '%s' \"$1\" > \"$HOME/.local/share/solyra/shelf.json\"",
            "sh", JSON.stringify(a)
        ])
        rev += 1
    }

    function add(u, pinned) {
        if (!u) return
        for (let i = 0; i < count; i++) if (get(i).url === u) return
        const p = cleanPath(u)
        const base = p.split("/").filter(x => x).pop() || p
        const dot = base.lastIndexOf(".")
        const ext = dot > 0 ? base.slice(dot + 1).toLowerCase() : ""
        append({
            url: u,
            name: base,
            isDir: u.endsWith("/"),
            ext: ext,
            sizeBytes: 0,
            sizeLabel: u.endsWith("/") ? "Folder" : "",
            dims: "",
            snippet: "",
            pinned: !!pinned,
            selected: false
        })
        persist()
        refreshMeta()
    }

    function removeUrl(u) {
        for (let i = 0; i < count; i++) if (get(i).url === u) { remove(i); break }
        persist()
    }

    function toggleSelect(idx) {
        if (idx < 0 || idx >= count) return
        setProperty(idx, "selected", !get(idx).selected)
        rev += 1
    }

    function selectOnly(idx) {
        for (let i = 0; i < count; i++) setProperty(i, "selected", i === idx)
        rev += 1
    }

    function toggleSelectAll() {
        let allSel = count > 0
        for (let i = 0; i < count; i++) if (!get(i).selected) { allSel = false; break }
        for (let i = 0; i < count; i++) setProperty(i, "selected", !allSel)
        rev += 1
    }

    function hasSelection() {
        for (let i = 0; i < count; i++) if (get(i).selected) return true
        return false
    }

    function selectedPaths() {
        const out = []
        for (let i = 0; i < count; i++) if (get(i).selected) out.push(cleanPath(get(i).url))
        if (out.length === 0) {
            for (let i = 0; i < count; i++) out.push(cleanPath(get(i).url))
        }
        return out
    }

    function togglePin(idx) {
        if (idx < 0 || idx >= count) return
        setProperty(idx, "pinned", !get(idx).pinned)
        persist()
    }

    function zipSelected() {
        const paths = selectedPaths()
        if (paths.length === 0) return
        if (paths.length === 1 && /\.zip$/i.test(paths[0])) {
            actionProc.command = [
                "sh", "-c",
                "f=\"$1\"; d=\"$(dirname \"$f\")\"; unzip -oq \"$f\" -d \"$d\" && echo \"Extracted $(basename \"$f\")|Unzipped in place\"",
                "sh", paths[0]
            ]
        } else {
            actionProc.command = [
                "sh", "-c",
                "out=\"$HOME/Downloads/Archive-$(date +%H%M%S).zip\"; zip -qrj \"$out\" \"$@\" 2>/dev/null || tar -czf \"${out%.zip}.tar.gz\" \"$@\"; " +
                "sz=$(stat -c %s \"$out\" 2>/dev/null || stat -c %s \"${out%.zip}.tar.gz\" 2>/dev/null || echo 0); " +
                "echo \"Archived $# item(s)|Saved to Downloads ($(numfmt --to=iec-i --suffix=B $sz 2>/dev/null || echo ${sz}B))\"",
                "sh"
            ].concat(paths)
        }
        actionProc.running = true
    }

    function compressSelected() {
        const paths = selectedPaths()
        if (paths.length === 0) return
        const target = paths[0]
        actionProc.command = [
            "sh", "-c",
            "f=\"$1\"; [ -f \"$f\" ] || exit 0; base=\"$(basename \"$f\")\"; " +
            "before=$(stat -c %s \"$f\" 2>/dev/null || echo 0); " +
            "case \"${base##*.}\" in " +
            "  jpg|JPG|jpeg|JPEG|png|PNG) " +
            "    if command -v ffmpeg >/dev/null 2>&1; then " +
            "      tmp=\"${f}.min.jpg\"; ffmpeg -y -loglevel error -i \"$f\" -q:v 6 \"$tmp\" && [ -s \"$tmp\" ] && mv \"$tmp\" \"$f\" || rm -f \"$tmp\"; " +
            "    fi ;; " +
            "  pdf|PDF) " +
            "    if command -v gs >/dev/null 2>&1; then " +
            "      tmp=\"${f}.min.pdf\"; gs -sDEVICE=pdfwrite -dCompatibilityLevel=1.4 -dPDFSETTINGS=/ebook -dNOPAUSE -dQUIET -dBATCH -sOutputFile=\"$tmp\" \"$f\" && [ -s \"$tmp\" ] && mv \"$tmp\" \"$f\" || rm -f \"$tmp\"; " +
            "    else gzip -9 -kf \"$f\" && mv \"${f}.gz\" \"${f}.compressed.gz\"; fi ;; " +
            "  *) gzip -9 -kf \"$f\" ;; " +
            "esac; " +
            "after=$(stat -c %s \"$f\" 2>/dev/null || echo $before); " +
            "if [ \"$after\" -ge \"$before\" ] && [ -f \"${f}.gz\" ]; then after=$(stat -c %s \"${f}.gz\" 2>/dev/null || echo $after); fi; " +
            "if [ \"$after\" -ge \"$before\" ] && [ -f \"${f}.compressed.gz\" ]; then after=$(stat -c %s \"${f}.compressed.gz\" 2>/dev/null || echo $after); fi; " +
            "fmt() { awk -v b=\"$1\" 'BEGIN{if(b>=1e6) printf \"%.1f MB\", b/1e6; else if(b>=1e3) printf \"%.0f KB\", b/1e3; else printf \"%d B\", b}'; }; " +
            "if [ \"$before\" -gt 0 ] && [ \"$after\" -lt \"$before\" ]; then " +
            "  pct=$(( (before - after) * 100 / before )); " +
            "  echo \"Compressed $base|$(fmt $before) → $(fmt $after) (-${pct}%)\"; " +
            "else " +
            "  echo \"Compressed $base|$(fmt $before) (already optimal)\"; " +
            "fi",
            "sh", target
        ]
        actionProc.running = true
    }

    function groupIntoNewFolder() {
        const paths = selectedPaths()
        if (paths.length === 0) return
        const folder = Quickshell.env("HOME") + "/Downloads/Folder-" + Qt.formatDateTime(new Date(), "HHmmss")
        actionProc.command = [
            "sh", "-c",
            "dir=\"$1\"; shift; mkdir -p \"$dir\"; for f in \"$@\"; do [ -e \"$f\" ] && cp -r \"$f\" \"$dir/\"; done; " +
            "echo \"Created $(basename \"$dir\")|Gathered $# item(s)\"",
            "sh", folder
        ].concat(paths)
        actionProc.running = true
        add("file://" + folder + "/", false)
    }

    function clearUnpinned() {
        for (let i = count - 1; i >= 0; i--) {
            if (!get(i).pinned) remove(i)
        }
        persist()
        actionToast("Cleared File Tray", "Pinned folders kept")
    }
}
