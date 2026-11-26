// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// Recent, Favorites or the Trash: items from many folders, each with the folder it is in (for
// the Trash, the one it was deleted from) and the list's own time.
ColumnLayout {
    id: view

    required property var browser
    required property var items
    property string whenTitle: qsTr("Modified")
    property string folderTitle: qsTr("Location")
    property string emptyText
    property int selectedRow: -1
    readonly property string status: selectedRow >= 0 ? qsTr("%n items", "", list.count) + " · " + qsTr("1 selected")
                                                      : qsTr("%n items", "", list.count)
    readonly property bool showFolder: list.width >= 640
    readonly property bool showKind: list.width >= 860
    // Opens a folder in this tab.
    property bool opensFolders: true
    // The Trash: a bar to restore or delete what is selected, or empty it all.
    property bool trashActions: false
    signal restoreRequested(int row)
    signal deleteRequested(int row)
    signal emptyRequested()
    /// A file was double-clicked or Entered: the window opens it.
    signal fileActivated(string path)
    /// A right click on `row`, selected first.
    signal contextRequested(int row)

    function focusList() { list.forceActiveFocus() }
    readonly property string currentPath: selectedRow >= 0 && items ? (items.count, items.pathAt(selectedRow)) : ""
    function step(delta) {
        if (list.count === 0) return
        selectedRow = Math.max(0, Math.min(list.count - 1, selectedRow + delta))
        list.positionViewAtIndex(selectedRow, ListView.Contain)
    }
    function open(row) {
        if (row < 0) return
        if (items.isFolderAt(row)) {
            if (opensFolders) browser.goPath(items.pathAt(row))
        } else if (opensFolders) {
            fileActivated(items.pathAt(row))
        }
    }

    // 10xer's keys over these items (search results, Recent, Favorites, the Trash): j / k and
    // paging, g g / G, l / Enter / o open, y / x yank or cut, g f shows the item in its folder,
    // h / Esc / Backspace go back to where it was opened from.
    property bool chordG: false
    function tenxerKey(event) {
        const mods = event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)
        if ([Qt.Key_Shift, Qt.Key_Control, Qt.Key_Alt, Qt.Key_Meta].includes(event.key)) return false
        const text = (event.modifiers & Qt.ShiftModifier) ? event.text.toUpperCase() : event.text
        const page = Math.max(1, Math.floor(list.height / 32))
        const to = (row) => {
            if (list.count === 0) return
            selectedRow = Math.max(0, Math.min(list.count - 1, row))
            list.positionViewAtIndex(selectedRow, ListView.Contain)
        }
        const w = view.Window.window
        if (chordG) {
            chordG = false
            if (text === "g") to(0)
            else if (text === "f" && selectedRow >= 0 && w) {
                // The folder holding it, with it selected.
                const path = items.pathAt(selectedRow)
                w.browser.goPath(path.substring(0, path.lastIndexOf("/")) || "/")
                w.revealItems([path], false)
            }
            return true
        }
        if (mods === Qt.ControlModifier) {
            if (event.key === Qt.Key_D) to(selectedRow + Math.floor(page / 2))
            else if (event.key === Qt.Key_U) to(selectedRow - Math.floor(page / 2))
            else if (event.key === Qt.Key_F) to(selectedRow + page)
            else if (event.key === Qt.Key_B) to(selectedRow - page)
            else return false
            return true
        }
        if (mods !== Qt.NoModifier) return false
        switch (event.key) {
        case Qt.Key_Down: to(selectedRow + 1); return true
        case Qt.Key_Up: to(selectedRow - 1); return true
        case Qt.Key_PageDown: to(selectedRow + page); return true
        case Qt.Key_PageUp: to(selectedRow - page); return true
        case Qt.Key_Home: to(0); return true
        case Qt.Key_End: to(list.count - 1); return true
        case Qt.Key_Return:
        case Qt.Key_Enter:
        case Qt.Key_Right: open(selectedRow); return true
        case Qt.Key_Escape:
        case Qt.Key_Left:
        case Qt.Key_Backspace: browser.back(); return true
        }
        switch (text) {
        case "j": to(selectedRow + 1); return true
        case "k": to(selectedRow - 1); return true
        case "G": to(list.count - 1); return true
        case "g": chordG = true; return true
        case "l":
        case "o": open(selectedRow); return true
        case "h": browser.back(); return true
        case "y":
        case "x":
            if (selectedRow >= 0 && w) w.ops.copyPaths([items.pathAt(selectedRow)], text === "x")
            return true
        }
        return text.length === 1 && text > " "
    }

    spacing: 0
    onItemsChanged: selectedRow = -1

    RowLayout {
        visible: view.trashActions
        Layout.fillWidth: true
        Layout.leftMargin: 16
        Layout.rightMargin: 16
        Layout.topMargin: 4
        spacing: 8
        Button { text: qsTr("Restore"); enabled: view.selectedRow >= 0; onClicked: view.restoreRequested(view.selectedRow) }
        Button { text: qsTr("Delete for Good…"); enabled: view.selectedRow >= 0; onClicked: view.deleteRequested(view.selectedRow) }
        Item { Layout.fillWidth: true }
        Button { text: qsTr("Empty Trash…"); enabled: list.count > 0; onClicked: view.emptyRequested() }
    }
    RowLayout {
        Layout.preferredHeight: 36
        Layout.leftMargin: 24
        spacing: 0
        Repeater {
            model: [[qsTr("Name"), 380, false, true], [view.folderTitle, 300, false, view.showFolder], [qsTr("Size"), 91, true, true], ["", 24, false, true], [qsTr("Kind"), 119, false, view.showKind], [view.whenTitle, 140, false, true]]
            Text {
                required property var modelData
                required property int index
                visible: modelData[3]
                Layout.fillWidth: index < 2
                Layout.maximumWidth: index === 0 ? 420 : modelData[1]
                Layout.preferredWidth: modelData[1]
                Layout.minimumWidth: index === 0 ? 160 : 0
                Layout.leftMargin: index === 1 ? 12 : 0
                horizontalAlignment: modelData[2] ? Text.AlignRight : Text.AlignLeft
                text: modelData[0]
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.tiny
                font.weight: Font.DemiBold
            }
        }
        Item { Layout.fillWidth: true }
    }
    ListView {
        id: list
        objectName: "itemList"
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.leftMargin: 8
        Layout.rightMargin: 8
        clip: true
        reuseItems: true
        focus: true
        model: view.items
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        delegate: ItemRow {
            width: ListView.view.width
            selected: index === view.selectedRow
            showFolder: view.showFolder
            showKind: view.showKind
            onClicked: { view.selectedRow = index; list.forceActiveFocus() }
            onActivated: view.open(index)
            onMenuRequested: { view.selectedRow = index; list.forceActiveFocus(); view.contextRequested(index) }
        }
        Keys.onPressed: (event) => { event.accepted = AppSettings.tenxer && view.tenxerKey(event) }
        Keys.onShortcutOverride: (event) => {
            event.accepted = AppSettings.tenxer && (event.key === Qt.Key_Backspace || event.key === Qt.Key_Escape
                || ((event.modifiers & Qt.ControlModifier) && [Qt.Key_B, Qt.Key_D, Qt.Key_F, Qt.Key_U].includes(event.key)))
        }
        Keys.onUpPressed: { view.selectedRow = Math.max(0, view.selectedRow - 1); positionViewAtIndex(view.selectedRow, ListView.Contain) }
        Keys.onDownPressed: { view.selectedRow = Math.min(count - 1, view.selectedRow + 1); positionViewAtIndex(view.selectedRow, ListView.Contain) }
        Keys.onReturnPressed: view.open(view.selectedRow)
        Text {
            anchors.centerIn: parent
            visible: list.count === 0
            text: view.emptyText
            color: Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.body
        }
    }
}
