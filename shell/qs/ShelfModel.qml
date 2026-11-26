import QtQuick
import Quickshell
import Quickshell.Io

// The file shelf: paths parked from drags, kept across sessions.
ListModel {
    id: shelf
    property var file: FileView {
        path: Quickshell.env("HOME") + "/.local/share/solyra/shelf.json"
        onLoaded: { try { for (const u of JSON.parse(text())) shelf.append({ url: u }) } catch (e) {} }
        onLoadFailed: {}
    }
    function urls() { const a = []; for (let i = 0; i < count; i++) a.push(get(i).url); return a }
    function persist() { file.setText(JSON.stringify(urls())) }
    function add(u) { for (let i = 0; i < count; i++) if (get(i).url === u) return; append({ url: u }); persist() }
    function removeUrl(u) { for (let i = 0; i < count; i++) if (get(i).url === u) { remove(i); break } persist() }
}
