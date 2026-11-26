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
    required property var compress
    required property var openWith
    /// The folder peek (i on a folder in List and Icons).
    required property var peek
    /// What `;` was pressed on: its actions run on these.
    property var actionTargets: []
    /// The actions offered for them, those at the menu's top first, ten at most.
    readonly property var numbered: chord === ";" ? window.topActions.concat(window.submenuActions).slice(0, 10) : []

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

    // The footer prompt: what it is for, what it says is wrong, the folders it offers.
    /// "find", "findBack", "filter", "search", "rename", "create", "move", "copy", "go", "jump",
    /// "recent", or "" when none is open.
    property string prompt: ""
    property string promptError: ""
    property int chosen: 0
    /// What the prompt was opened on: the items to move or copy, the row to rename.
    property var fixed: []
    property int fixedRow: -1
    property string findQuery: ""
    /// What the names show marked: the last find, until Esc.
    property string found: ""
    property bool findBackward: false
    readonly property bool picking: ["go", "move", "copy", "extract"].includes(prompt)
    property var listed: []
    readonly property var candidates: picking ? picker.folders.filter(path => acceptable(path)) : listed
    readonly property string promptLabel: ({ find: "/", findBack: "?", filter: qsTr("filter ›"), search: qsTr("search ›"),
                                             rename: qsTr("rename ›"), create: qsTr("create ›"), move: qsTr("move to ›"),
                                             copy: qsTr("copy to ›"), go: qsTr("go ›"), jump: qsTr("jump ›"),
                                             recent: qsTr("recent ›"), extract: qsTr("extract to ›") })[prompt] || ""
    readonly property string promptStatus: promptError.length > 0 ? promptError
        : picking && picker.searching && candidates.length === 0 ? qsTr("Searching…")
        : (picking || prompt === "jump" || prompt === "recent") && candidates.length === 0 && (picking ? picker.message : qsTr("No matching folders")).length > 0
            ? (picking ? picker.message : qsTr("No matching folders"))
        : candidates.length > 1 ? qsTr("%1 of %2").arg(chosen + 1).arg(candidates.length) : ""
    /// The prompt opens with `text`, `from`–`to` of it selected.
    signal promptOpened(string text, int from, int to)

    FolderPicker { id: picker; visits: keys.window.visits }

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
        : chord === ";" ? numbered.map((action, index) => ({ key: String((index + 1) % 10), label: action.name })).concat([
              { key: "t", label: qsTr("terminal here") }, { key: "c", label: qsTr("compress") },
              { key: "e", label: qsTr("extract here") }, { key: "E", label: qsTr("extract to…") }])
        : []
    readonly property string visualLabel: visual > 0 ? qsTr("VISUAL") : visual < 0 ? qsTr("UNSET") : ""

    function say(message) {
        flash = message
        flashTimer.restart()
    }
    Timer { id: flashTimer; interval: 2400; onTriggered: keys.flash = "" }

    // Everything the mode holds goes when it is turned off.
    function reset() {
        peek.close()
        closePrompt()
        findQuery = ""
        found = ""
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
        function onFolderChanged() {
            keys.visual = 0
            keys.peek.close()
        }
        // The cursor starts on the first item, or the folder just left.
        function onGenerationChanged() {
            const view = keys.window.view
            if (!AppSettings.tenxer || !view || !view.cursorRow) return
            if (keys.land()) return
            if (view.cursorRow() < 0 && view.rowCount() > 0) view.setCursorRow(0)
        }
    }
    /// Puts the cursor on `landOn` if it shows yet.
    function land() {
        const view = window.view
        if (landOn.length === 0 || !view || !view.cursorRow) return false
        const row = window.folderModel.rowOfName(landOn)
        if (row < 0) return false
        view.setCursorRow(row)
        landOn = ""
        return true
    }
    // An item made or renamed shows a moment later, however its row arrives.
    Timer {
        property int tries: 0
        interval: 100
        repeat: true
        running: keys.landOn.length > 0
        onRunningChanged: tries = 0
        onTriggered: if (keys.land() || ++tries > 30) { keys.landOn = ""; stop() }
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
        if (keys.window.chooser !== null && chooserRefuses(event, mods)) return true
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
        case Qt.Key_F2: rename(view); return true
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
        // The open preview scrolls from here, the keys staying in the files.
        case "J":
            if (keys.window.previewOpen && keys.window.previewView) keys.window.previewView.scroll(3)
            return true
        case "K":
            if (keys.window.previewOpen && keys.window.previewView) keys.window.previewView.scroll(-3)
            return true
        case "L": keys.window.browser.forward(); return true
        case "o": openItem(view); return true
        case "i": peekAt(view); return true
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
        case ";":
            chord = ";"
            actionTargets = targets(view)
            keys.window.actions.offer(actionTargets)
            return true
        case "O":
            if (targets(view).length === 0) say(qsTr("Nothing to open"))
            else openWith.ask(targets(view))
            return true
        case "R": say(view.rowCount() === 0 ? qsTr("Nothing to restore") : qsTr("Only items in Trash can be restored")); return true
        case "/": openPrompt(view, "find", ""); return true
        case "?": openPrompt(view, "findBack", ""); return true
        case "n": findNext(view, findBackward); return true
        case "N": findNext(view, !findBackward); return true
        case "f": openPrompt(view, "filter", view.folderModel.filter); return true
        case "s": openPrompt(view, "search", ""); return true
        case "z": openPrompt(view, "jump", ""); return true
        case "Z": openPrompt(view, "recent", ""); return true
        case "a": openPrompt(view, "create", ""); return true
        case "r": rename(view); return true
        case "M": sendTo(view, true); return true
        case "C": sendTo(view, false); return true
        }
        // Other plain letters do nothing here (no typing to filter in this mode).
        return event.text.length === 1 && event.text > " "
    }

    // The file chooser chooses files: it doesn't copy, move or restore them, change pins, open
    // them with an application or run actions; one-item requests select one.
    function chooserRefuses(event, mods) {
        const text = typed(event)
        const single = !keys.window.chooser.multiple
        if (mods === Qt.ControlModifier) {
            if ([Qt.Key_C, Qt.Key_X, Qt.Key_V].includes(event.key)) { say(qsTr("Not available in the file chooser")); return true }
            if (single && [Qt.Key_A, Qt.Key_R].includes(event.key)) { say(qsTr("Only one item can be chosen")); return true }
            return false
        }
        if (mods !== Qt.NoModifier) return false
        if (["y", "x", "p", "P", "Y", "X", "M", "C", "R", "O", ";", "i", "Q"].includes(text)) {
            say(qsTr("Not available in the file chooser"))
            return true
        }
        if (single && [" ", "v", "V"].includes(text)) { say(qsTr("Only one item can be chosen")); return true }
        return false
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
        // The peek is about the item the cursor was on.
        if (peek.opened) peek.close()
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
        if (peek.opened) { peek.close(); return }
        if (visual !== 0) { visual = 0; return }
        if (found.length > 0) { found = ""; return }
        if (view.folderModel.filter.length > 0) { view.folderModel.filter = ""; return }
        if (keys.window.previewOpen) { keys.window.previewOpen = false; return }
        if (view.folderModel.selectionCount > 0) { view.folderModel.clearSelection(); return }
        // Nothing left to dismiss: a chooser is cancelled.
        if (keys.window.chooser !== null) keys.window.chooser.cancel()
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
        if (isFolderAt(view, cursor)) {
            openFolder(view, cursor)
            return
        }
        // Opened if it isn't, and given the keys (where the preview is a panel; a popup has them).
        keys.window.previewOpen = true
        Qt.callLater(() => {
            const preview = keys.window.previewView
            if (preview && keys.window.previewAsPanel) {
                preview.owning = true
                preview.forceActiveFocus()
            }
        })
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
        const chooser = keys.window.chooser
        // A Save request saves the name in this folder, whatever the cursor is on.
        if (chooser !== null && chooser.kind !== "open") { keys.window.chooserAccept(false); return }
        if (cursor < 0) return
        if (isFolderAt(view, cursor)) openFolder(view, cursor)
        else if (chooser !== null) {
            // The fill, or the cursor's file.
            if (view.folderModel.selectionCount === 0) view.folderModel.select(cursor)
            keys.window.chooserAccept(false)
        }
        else view.activate(cursor)
    }
    function peekAt(view) {
        const cursor = view.cursorRow()
        if (cursor < 0) return
        if (!isFolderAt(view, cursor)) {
            keys.window.previewOpen = !keys.window.previewOpen
            return
        }
        // Columns: the folder opens in the next column.
        if (view.mode === 2) { openFolder(view, cursor); return }
        if (peek.opened) { peek.close(); return }
        const item = view.current.itemAtIndex(cursor)
        const at = item ? item.mapToItem(peek.parent, 40, item.height) : Qt.point(peek.parent.width / 3, peek.parent.height / 3)
        peek.show(view.folderModel.pathAt(cursor), at)
    }

    // Verbs take the fill, or the cursor's item when nothing is filled.
    function targets(view) {
        if (view.folderModel.selectionCount > 0) return view.folderModel.selectedPaths()
        const cursor = view.cursorRow()
        // A row not read yet has no path to act on.
        const path = cursor >= 0 ? view.folderModel.pathAt(cursor) : ""
        return path.length > 0 ? [path] : []
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
        else if (armed === ";") actionChord(text, view)
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
        if (text === "+" || text === "-") {
            if (window.chooser !== null) say(qsTr("Not available in the file chooser"))
            else pin(view, text === "+")
            return
        }
        if (text === " ") { openPrompt(view, "go", ""); return }
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

    // Prompts. The bar shows one and sends its keys back here.
    function openPrompt(view, kind, text) {
        chord = ""
        promptError = ""
        chosen = 0
        listed = []
        prompt = kind
        edited(text)
        promptOpened(text, 0, text.length)
    }
    function closePrompt() {
        picker.cancel()
        prompt = ""
        promptError = ""
        listed = []
        fixed = []
        fixedRow = -1
    }
    /// Leaves the prompt, back to the files (the filter stays as typed).
    function cancelPrompt() {
        if (prompt === "filter") window.folderModel.filter = ""
        closePrompt()
        window.view.focusList()
    }
    function edited(text) {
        promptError = ""
        chosen = 0
        const here = window.browser.page === 0 ? window.browser.location : ""
        if (prompt === "filter") window.folderModel.filter = text
        else if (prompt === "jump" && window.visits) listed = window.visits.jump(text, here)
        else if (prompt === "recent" && window.visits) listed = window.visits.recent(text, here)
        else if (picking) {
            // Addresses are never searched or probed before Enter.
            if (prompt === "go" && /^[a-z][a-z0-9+.-]*:|^\/\/|^\\\\|^[^\s\/]+@[^\s\/]+:/i.test(text)) picker.cancel()
            else picker.search(text, here, window.folderModel.showHidden)
        }
    }
    function choose(step) {
        const count = candidates.length
        if (count > 0) chosen = (chosen + step + count) % count
    }
    /// Tab: the chosen folder written into the prompt, to search inside it.
    function written() {
        const path = candidates[chosen]
        if (path === undefined) return null
        const here = window.browser.location
        const home = keys.window.places.standardFolder("home")
        const shown = path.startsWith(here + "/") ? "./" + path.substring(here.length + 1)
                    : path.startsWith(home + "/") ? "~/" + path.substring(home.length + 1) : path
        return shown.endsWith("/") ? shown : shown + "/"
    }
    // A folder a move or copy can go to: not where the items are, not one of them or inside one.
    function acceptable(path) {
        if (prompt !== "move" && prompt !== "copy") return true
        for (const source of fixed) {
            if (path === source || path.startsWith(source + "/")) return false
        }
        return !(prompt === "move" && fixed.length > 0 && path === fixed[0].substring(0, fixed[0].lastIndexOf("/")))
    }
    function submit(text) {
        const view = window.view
        const kind = prompt
        if (kind === "find" || kind === "findBack") {
            closePrompt()
            view.focusList()
            if (text.length === 0) return
            findQuery = text
            findBackward = kind === "findBack"
            findNext(view, findBackward)
        } else if (kind === "filter") {
            closePrompt()
            view.focusList()
            if (view.rowCount() > 0) view.setCursorRow(0)
        } else if (kind === "search") {
            closePrompt()
            if (text.trim().length > 0) window.searchHere(text)
            else view.focusList()
        } else if (kind === "jump" || kind === "recent") {
            const path = candidates[chosen]
            if (path === undefined) { promptError = qsTr("No matching folders"); return }
            closePrompt()
            view.browser.goPath(path)
        } else if (kind === "go") {
            const path = candidates[chosen]
            closePrompt()
            if (path !== undefined && picker.kind(picker.resolve(text, view.browser.location)) === "") view.browser.goPath(path)
            else if (text.trim().length > 0) view.browser.goTyped(text.trim())
            view.focusList()
        } else if (kind === "move" || kind === "copy") {
            submitTransfer(text, kind === "move")
        } else if (kind === "extract") {
            submitExtract(text)
        } else if (kind === "rename") {
            submitRename(text)
        } else if (kind === "create") {
            submitCreate(text)
        }
    }
    function submitTransfer(text, move) {
        if (text.trim().length === 0 && candidates.length === 0) { closePrompt(); window.view.focusList(); return }
        const typed = picker.resolve(text, window.browser.location)
        // A path typed out names the folder; else the one chosen.
        let into = typed.length > 0 && picker.kind(typed) !== "" ? typed : candidates[chosen]
        if (into === undefined) into = typed
        if (picker.kind(into) === "") { promptError = qsTr("No such folder"); return }
        if (picker.kind(into) !== "folder") { promptError = qsTr("Not a folder"); return }
        if (move && fixed.length > 0 && into === fixed[0].substring(0, fixed[0].lastIndexOf("/"))) { promptError = qsTr("Already in this folder"); return }
        for (const source of fixed) {
            if (into === source || into.startsWith(source + "/")) { promptError = qsTr("Can’t put a folder inside itself"); return }
        }
        const paths = fixed
        closePrompt()
        window.ops.transferPaths(paths, into, move)
        window.view.focusList()
    }
    function rename(view) {
        const row = view.cursorRow()
        if (row < 0) { say(qsTr("Nothing to rename")); return }
        const path = view.folderModel.pathAt(row)
        const name = path.substring(path.lastIndexOf("/") + 1)
        openPrompt(view, "rename", name)
        fixedRow = row
        // A file's stem is selected, a folder's whole name.
        const dot = view.folderModel.isFolder(row) ? -1 : name.lastIndexOf(".")
        promptOpened(name, 0, dot > 0 ? dot : name.length)
    }
    function submitRename(text) {
        const model = window.folderModel
        const path = model.pathAt(fixedRow)
        if (text === path.substring(path.lastIndexOf("/") + 1)) { closePrompt(); window.view.focusList(); return }
        const why = nameError(text)
        if (why.length > 0) { promptError = why; return }
        model.rename(fixedRow, text)
    }
    function nameError(name) {
        if (name.length === 0) return qsTr("Enter a name")
        if (name === "." || name === "..") return qsTr("“%1” can’t be a name").arg(name)
        if (name.includes("/")) return qsTr("Names can’t contain “/”")
        return ""
    }
    function submitCreate(text) {
        const folder = text.endsWith("/")
        const name = folder ? text.substring(0, text.length - 1) : text
        const why = nameError(name)
        if (why.length > 0) { promptError = why; return }
        window.ops.createNamed(window.folderModel, name, folder)
    }
    function sendTo(view, move) {
        const paths = targets(view)
        if (paths.length === 0) { say(move ? qsTr("Nothing to move") : qsTr("Nothing to copy")); return }
        openPrompt(view, move ? "move" : "copy", "")
        fixed = paths
        edited("")
    }
    // / and ?: the next name holding the text, from the cursor, wrapping.
    function findNext(view, backward) {
        if (findQuery.length === 0) return
        found = findQuery
        const count = view.rowCount()
        const from = view.cursorRow()
        const query = findQuery.toLowerCase()
        for (let step = 1; step <= count; ++step) {
            const row = ((from + (backward ? -step : step)) % count + count) % count
            const path = view.folderModel.pathAt(row)
            if (path.substring(path.lastIndexOf("/") + 1).toLowerCase().includes(query)) {
                move(view, row)
                return
            }
        }
        say(qsTr("No matches for “%1”").arg(findQuery))
    }
    Connections {
        target: keys.window.folderModel
        function onRenamed(name) {
            if (keys.prompt !== "rename") return
            keys.closePrompt()
            keys.window.view.focusList()
        }
        function onFailed(message) { if (keys.prompt === "rename") keys.promptError = message }
    }
    Connections {
        target: keys.window.ops
        function onMade(folder, name) {
            if (keys.prompt !== "create") return
            keys.closePrompt()
            keys.landOn = name
            keys.window.view.focusList()
        }
        function onMakeRefused(why) { if (keys.prompt === "create") keys.promptError = why }
    }

    // ; : the custom actions offered for the targets, a terminal, compressing and extracting.
    function actionChord(text, view) {
        if (text >= "0" && text <= "9") {
            const index = text === "0" ? 9 : Number(text) - 1
            const action = numberedAt(index)
            if (action === undefined) { say(qsTr("No action %1").arg(text)); return }
            if (JSON.stringify(targets(view)) !== JSON.stringify(actionTargets)) { say(qsTr("Selection changed")); return }
            window.runAction(action, actionTargets, view.browser.location, false)
        } else if (text === "t") {
            const why = view.browser.page === 0 ? window.apps.openTerminal(view.browser.location) : qsTr("Can’t open a terminal here")
            if (why.length > 0) say(why)
        } else if (text === "c") {
            const paths = targets(view)
            if (paths.length === 0) say(qsTr("Nothing to compress"))
            else compress.ask(paths)
        } else if (text === "e" || text === "E") {
            const archive = oneArchive(view)
            if (archive.length === 0) return
            if (text === "e") {
                window.ops.extract(archive)
            } else {
                openPrompt(view, "extract", "")
                fixed = [archive]
            }
        } else say(qsTr("Unknown chord"))
    }
    // The chord's list was read when ; was pressed; the digit reads it again.
    function numberedAt(index) {
        return window.topActions.concat(window.submenuActions).slice(0, 10)[index]
    }
    function oneArchive(view) {
        const paths = targets(view)
        if (paths.length === 0) { say(qsTr("Nothing to extract")); return "" }
        if (paths.length > 1) { say(qsTr("Extract one archive at a time")); return "" }
        if (window.ops.archiveFormat(paths[0]) === "") { say(qsTr("Not an archive")); return "" }
        return paths[0]
    }
    function submitExtract(text) {
        const typed = picker.resolve(text, window.browser.location)
        let into = typed.length > 0 && picker.kind(typed) !== "" ? typed : candidates[chosen]
        if (into === undefined) into = typed
        if (picker.kind(into) === "") { promptError = qsTr("No such folder"); return }
        if (picker.kind(into) !== "folder") { promptError = qsTr("Not a folder"); return }
        const archive = fixed[0]
        closePrompt()
        window.ops.extract(archive, into)
        window.view.focusList()
    }
}
