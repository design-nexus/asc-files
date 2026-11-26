// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles
import AscFiles.System

// 10xer mode's keys (Ctrl+Shift+M): a keyboard-first, Yazi-style map over the folder shown. The
// cursor moves apart from the selection's fill; v / V walk ranges; verbs act on the fill, or
// the cursor's item when nothing is filled; g, t, comma and c start two-key chords, shown in the
// footer with their second keys. The window's views send their keys here first.
Item {
    id: keys

    required property var window
    required property var confirm

    /// The chord armed: "g", "t", ",", "c", or "".
    property string chord: ""
    /// A range being walked: 1 adding (v), -1 taking out (V), 0 none.
    property int visual: 0
    property int anchor: -1
    property var base: []
    /// A short message in the footer: what a key couldn't do.
    property string flash: ""
    /// A folder to put the cursor on once it shows (the one left by going up).
    property string landOn: ""

    readonly property var chordOptions: chord === "g" ? [
        { key: "g", label: qsTr("first item") }, { key: "h", label: qsTr("Home") }, { key: "d", label: qsTr("Downloads") },
        { key: "k", label: qsTr("Documents") }, { key: "p", label: qsTr("Pictures") }, { key: "v", label: qsTr("Videos") },
        { key: "c", label: qsTr("Config") }, { key: "r", label: qsTr("Recent") }, { key: "t", label: qsTr("Trash") },
        { key: "1–9", label: qsTr("pinned folders") }, { key: "+ / -", label: qsTr("pin / unpin") }]
        : chord === "t" ? [{ key: "n", label: qsTr("new tab") }, { key: "x", label: qsTr("close tab") },
                           { key: "t", label: qsTr("previous tab") }, { key: "1–9, 0", label: qsTr("tab 1–10") }]
        : chord === "," ? [{ key: "a", label: qsTr("sort by name") }, { key: "m", label: qsTr("sort by modified") },
                           { key: "s", label: qsTr("sort by size") }, { key: "e", label: qsTr("sort by type") },
                           { key: "Shift", label: qsTr("reversed") }]
        : chord === "c" ? [{ key: "c", label: qsTr("copy path") }, { key: "n", label: qsTr("copy name") }]
        : []
    readonly property string visualLabel: visual > 0 ? qsTr("VISUAL") : visual < 0 ? qsTr("UNSET") : ""

    function say(message) {
        flash = message
        flashTimer.restart()
    }
    Timer { id: flashTimer; interval: 2400; onTriggered: keys.flash = "" }

    // Everything the mode holds goes when it is turned off.
    function reset() {
        chord = ""
        visual = 0
        flash = ""
        landOn = ""
    }
    Connections {
        target: AppSettings
        function onChanged() {
            if (!AppSettings.tenxer) {
                keys.reset()
                return
            }
            // Turned on in a folder already shown: the cursor starts on its first item.
            const view = keys.window.view
            if (view && view.cursorRow && view.cursorRow() < 0 && view.rowCount() > 0) view.setCursorRow(0)
        }
    }
    Connections {
        target: keys.window.folderModel
        function onFolderChanged() { keys.visual = 0 }
        // The cursor starts on the first item, or the folder just left.
        function onGenerationChanged() {
            const view = keys.window.view
            if (!AppSettings.tenxer || !view || !view.cursorRow) return
            if (keys.landOn.length > 0) {
                const row = keys.window.folderModel.rowOfName(keys.landOn)
                if (row >= 0) {
                    view.setCursorRow(row)
                    keys.landOn = ""
                    return
                }
            }
            if (view.cursorRow() < 0 && view.rowCount() > 0) view.setCursorRow(0)
        }
    }

    // Keys the mode takes from the window's shortcuts while a view has the keyboard.
    function claims(event) {
        const mods = event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.ShiftModifier | Qt.MetaModifier)
        if (mods === Qt.ControlModifier)
            return [Qt.Key_A, Qt.Key_B, Qt.Key_D, Qt.Key_F, Qt.Key_N, Qt.Key_R, Qt.Key_U, Qt.Key_V].includes(event.key)
        if (mods === Qt.ShiftModifier && event.key === Qt.Key_Delete) return true
        if (mods !== Qt.NoModifier && mods !== Qt.ShiftModifier) return false
        return event.key === Qt.Key_Space || event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete || chord.length > 0
    }

    /// What a key types, a letter in capitals with Shift whatever the layout reported.
    function typed(event) {
        return (event.modifiers & Qt.ShiftModifier) ? event.text.toUpperCase() : event.text
    }

    /// Handles a key pressed in `view`; false leaves it to the view and the window.
    function key(event, view) {
        if ([Qt.Key_Shift, Qt.Key_Control, Qt.Key_Alt, Qt.Key_Meta, Qt.Key_CapsLock].includes(event.key)) return false
        const mods = event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)
        if (chord.length > 0) {
            const armed = chord
            chord = ""
            if (event.key !== Qt.Key_Escape) chordKey(armed, event, view)
            return true
        }
        if (mods === Qt.ControlModifier) return control(event, view)
        if (mods !== Qt.NoModifier) return false
        const icons = view.mode === 1
        const cursor = view.cursorRow()
        switch (event.key) {
        case Qt.Key_Down: move(view, cursor + view.perRow); return true
        case Qt.Key_Up: move(view, Math.max(0, cursor - view.perRow)); return true
        case Qt.Key_Left: icons ? move(view, Math.max(0, cursor - 1)) : leave(view); return true
        case Qt.Key_Right: icons ? move(view, cursor + 1) : enter(view); return true
        case Qt.Key_Home: move(view, 0); return true
        case Qt.Key_End: move(view, view.rowCount() - 1); return true
        case Qt.Key_PageDown: move(view, cursor + view.pageRows()); return true
        case Qt.Key_PageUp: move(view, Math.max(0, cursor - view.pageRows())); return true
        case Qt.Key_Return:
        case Qt.Key_Enter: openItem(view); return true
        case Qt.Key_Escape: stepBack(view); return true
        case Qt.Key_Backspace: up(view); return true
        case Qt.Key_Delete: (event.modifiers & Qt.ShiftModifier) ? deleteForever(view) : trash(view); return true
        }
        switch (typed(event)) {
        case "j": move(view, cursor + view.perRow); return true
        case "k": move(view, Math.max(0, cursor - view.perRow)); return true
        case "h": icons ? move(view, Math.max(0, cursor - 1)) : leave(view); return true
        case "l": icons ? move(view, cursor + 1) : enter(view); return true
        case "G": move(view, view.rowCount() - 1); return true
        case "H": keys.window.browser.back(); return true
        case "L": keys.window.browser.forward(); return true
        case "o": openItem(view); return true
        case "i": peek(view); return true
        case " ": space(view); return true
        case "v": startVisual(view, 1); return true
        case "V": startVisual(view, -1); return true
        case "y": yank(view, false); return true
        case "x": yank(view, true); return true
        case "p":
        case "P": pasteHere(); return true
        case "Y":
        case "X": keys.window.ops.clearClipboard(); say(qsTr("Clipboard cleared")); return true
        case "d": trash(view); return true
        case "D": deleteForever(view); return true
        case ".": view.folderModel.showHidden = !view.folderModel.showHidden; return true
        case "Q": keys.window.close(); return true
        case "+":
        case "=": if (icons) view.browser.zoom(1); return true
        case "-": if (icons) view.browser.zoom(-1); return true
        case "g":
        case "t":
        case ",":
        case "c": chord = typed(event); return true
        }
        // Other plain letters do nothing here (no typing to filter in this mode).
        return event.text.length === 1 && event.text > " "
    }

    function control(event, view) {
        const cursor = view.cursorRow()
        const half = Math.max(1, Math.floor(view.pageRows() / 2))
        switch (event.key) {
        case Qt.Key_D: move(view, cursor + half); return true
        case Qt.Key_U: move(view, Math.max(0, cursor - half)); return true
        case Qt.Key_F: move(view, cursor + view.pageRows()); return true
        case Qt.Key_B: move(view, Math.max(0, cursor - view.pageRows())); return true
        case Qt.Key_A:
            if (view.rowCount() === 0) { say(qsTr("Nothing to select")); return true }
            visual = 0
            view.folderModel.selectAll()
            return true
        case Qt.Key_R:
            if (view.rowCount() === 0) { say(qsTr("Nothing to select")); return true }
            visual = 0
            view.folderModel.invertSelection()
            return true
        case Qt.Key_V: pasteHere(); return true
        case Qt.Key_N: keys.window.places.stepMode(); return true
        }
        return false
    }

    // Motion: the cursor, and a range being walked with it.
    function move(view, to) {
        const count = view.rowCount()
        if (count === 0) return
        const row = Math.max(0, Math.min(count - 1, to))
        view.setCursorRow(row)
        if (visual !== 0) walk(view, row)
    }
    function walk(view, row) {
        const model = view.folderModel
        const range = []
        for (let r = Math.min(anchor, row); r <= Math.max(anchor, row); ++r) range.push(r)
        model.clearSelection()
        model.setRowsSelected(base, true)
        model.setRowsSelected(range, visual > 0)
    }
    function startVisual(view, sign) {
        if (view.rowCount() === 0) { say(qsTr("Nothing to select")); return }
        if (visual === sign) { visual = 0; return }
        let cursor = view.cursorRow()
        if (cursor < 0) { cursor = 0; view.setCursorRow(0) }
        base = view.folderModel.selectedRows()
        anchor = cursor
        visual = sign
        walk(view, cursor)
    }
    function space(view) {
        const cursor = view.cursorRow()
        if (cursor < 0) { say(qsTr("Nothing to select")); return }
        view.folderModel.toggle(cursor)
        if (visual !== 0) {
            // Toggled inside a range, it stays as toggled when the range moves on.
            const at = base.indexOf(cursor)
            if (at >= 0) base.splice(at, 1)
            if (view.folderModel.isSelected(cursor)) base.push(cursor)
            return
        }
        move(view, cursor + 1)
    }
    // Esc: one step back per press.
    function stepBack(view) {
        if (visual !== 0) { visual = 0; return }
        if (view.folderModel.filter.length > 0) { view.folderModel.filter = ""; return }
        if (keys.window.previewOpen) { keys.window.previewOpen = false; return }
        view.folderModel.clearSelection()
    }

    // Going in and out.
    function isFolderAt(view, row) { return row >= 0 && view.folderModel.isFolder(row) }
    function openFolder(view, row) {
        if (view.mode === 2) view.browser.goColumnRow(view.folderModel, row)
        else view.browser.goRow(row)
    }
    function enter(view) {
        const cursor = view.cursorRow()
        if (cursor < 0) { say(qsTr("Nothing to preview")); return }
        if (isFolderAt(view, cursor)) openFolder(view, cursor)
        else keys.window.previewOpen = true
    }
    function leave(view) {
        if (keys.window.previewOpen) { keys.window.previewOpen = false; return }
        up(view)
    }
    function up(view) {
        const here = view.browser.location
        if (!view.browser.canGoUp) return
        landOn = here.substring(here.lastIndexOf("/") + 1)
        view.browser.up()
    }
    function openItem(view) {
        const cursor = view.cursorRow()
        if (cursor < 0) return
        if (isFolderAt(view, cursor)) openFolder(view, cursor)
        else view.activate(cursor)
    }
    function peek(view) {
        if (view.cursorRow() < 0) return
        keys.window.previewOpen = !keys.window.previewOpen
    }

    // Verbs take the fill, or the cursor's item when nothing is filled.
    function targets(view) {
        if (view.folderModel.selectionCount > 0) return view.folderModel.selectedPaths()
        const cursor = view.cursorRow()
        return cursor >= 0 ? [view.folderModel.pathAt(cursor)] : []
    }
    function yank(view, cut) {
        const paths = targets(view)
        if (paths.length === 0) { say(cut ? qsTr("Nothing to cut") : qsTr("Nothing to yank")); return }
        keys.window.ops.copyPaths(paths, cut)
    }
    function pasteHere() {
        if (!keys.window.ops.canPaste) { say(qsTr("Nothing to paste")); return }
        keys.window.pasteHere()
    }
    function trash(view) {
        const paths = targets(view)
        if (paths.length === 0) { say(qsTr("Nothing to delete")); return }
        confirm.ask(qsTr("Move %n items to the Trash?", "", paths.length), qsTr("They can be put back from the Trash."),
                    qsTr("Move to Trash"), () => keys.window.ops.trashPaths(paths))
        confirm.confirmKey = Qt.Key_D
    }
    function deleteForever(view) {
        const model = view.folderModel
        const cursor = view.cursorRow()
        if (model.selectionCount === 0 && cursor < 0) { say(qsTr("Nothing to delete")); return }
        const count = model.selectionCount > 0 ? model.selectionCount : 1
        confirm.ask(qsTr("Delete %n items for good?", "", count), qsTr("They will not go to the Trash, and this cannot be undone."),
                    qsTr("Delete"), () => {
                        if (model.selectionCount === 0) model.select(cursor)
                        keys.window.ops.deleteForever(model)
                    })
    }

    // The second key of a chord.
    function chordKey(armed, event, view) {
        const text = typed(event)
        if (armed === "g") goChord(text, view)
        else if (armed === "t") tabChord(text)
        else if (armed === ",") sortChord(text, view)
        else if (armed === "c") copyChord(text, view)
    }
    function goChord(text, view) {
        const folders = { h: "home", d: "downloads", k: "documents", p: "pictures", v: "videos", c: "config" }
        const missing = { d: qsTr("No Downloads folder"), k: qsTr("No Documents folder"), p: qsTr("No Pictures folder"),
                          v: qsTr("No Videos folder"), c: qsTr("No config folder"), h: qsTr("No Home folder") }
        if (text === "g") { move(view, 0); return }
        if (folders[text] !== undefined) {
            const path = keys.window.places.standardFolder(folders[text])
            if (path.length === 0) say(missing[text])
            else view.browser.goPath(path)
            return
        }
        // Pages: Recent and Trash.
        if (text === "r") { view.browser.goPage(2); return }
        if (text === "t") { view.browser.goPage(4); return }
        if (text >= "1" && text <= "9") {
            const pins = keys.window.places.pinnedPaths()
            const index = Number(text) - 1
            if (index < pins.length) view.browser.goPath(pins[index])
            else say(qsTr("No pin %1").arg(text))
            return
        }
        if (text === "+" || text === "-") { pin(view, text === "+"); return }
        say(qsTr("Unknown chord"))
    }
    function pin(view, adding) {
        const cursor = view.cursorRow()
        const path = isFolderAt(view, cursor) ? view.folderModel.pathAt(cursor) : view.browser.location
        const name = path.substring(path.lastIndexOf("/") + 1) || path
        const places = keys.window.places
        if (adding) {
            if (places.isPinned(path)) { say(qsTr("“%1” is already pinned").arg(name)); return }
            if (places.hasOwnPlace(path)) { say(qsTr("Can’t pin “%1”").arg(name)); return }
            places.pin(path)
            const index = places.pinnedPaths().indexOf(path)
            say(index >= 0 && index < 9 ? qsTr("Pinned “%1” as g %2").arg(name).arg(index + 1) : qsTr("Pinned “%1”").arg(name))
        } else {
            if (!places.isPinned(path)) { say(qsTr("“%1” isn’t pinned").arg(name)); return }
            places.unpin(path)
        }
    }
    function tabChord(text) {
        const tabs = keys.window.tabs
        if (text === "n") tabs.openTab(keys.window.browser.location)
        else if (text === "x") tabs.count > 1 ? tabs.closeTab(tabs.currentIndex) : keys.window.close()
        else if (text === "t") tabs.previous()
        else if (text >= "0" && text <= "9") {
            const index = text === "0" ? 9 : Number(text) - 1
            if (index < tabs.count) tabs.currentIndex = index
            else say(qsTr("No tab %1").arg(index + 1))
        } else say(qsTr("Unknown chord"))
    }
    function sortChord(text, view) {
        const sorts = { a: "name", m: "modified", s: "size", e: "type" }
        const key = sorts[text.toLowerCase()]
        if (key === undefined) { say(qsTr("Unknown chord")); return }
        view.browser.sortExactly(key, text !== text.toLowerCase())
    }
    function copyChord(text, view) {
        if (text !== "c" && text !== "n") { say(qsTr("Unknown chord")); return }
        const paths = targets(view)
        if (paths.length === 0) { say(qsTr("Nothing to copy")); return }
        keys.window.ops.copyPathText(text === "c" ? paths : paths.map(path => path.substring(path.lastIndexOf("/") + 1)))
    }
}
