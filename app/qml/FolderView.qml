// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// A folder, in the view the tab has chosen for it: List (columns you can sort by), Icons, or
// Columns. Clicks select (Ctrl adds or removes, Shift extends), a box dragged from empty space
// selects what it covers, and a double click or Enter opens a folder.
Item {
    id: view

    required property var browser
    required property var transfers
    required property var ops
    /// A right click: on `row` (selected now if it was not), or on empty space (-1).
    signal contextRequested(int row)
    readonly property var folderModel: browser ? browser.folderModel : null
    readonly property int mode: browser ? browser.view : 0
    readonly property int selectionCount: folderModel ? folderModel.selectionCount : 0
    readonly property string status: {
        const items = qsTr("%n items", "", list.count)
        return selectionCount > 0 ? items + " · " + qsTr("%n selected", "", selectionCount) : items
    }
    // The item the keyboard is on, for the preview; read again as the folder or selection changes.
    readonly property string currentPath: folderModel ? (folderModel.generation, folderModel.selectionCount,
        folderModel.pathAt(mode === 1 ? grid.currentIndex : list.currentIndex)) : ""
    // Moves to the next or previous item (`delta` rows), as the preview's arrows do.
    function step(delta) {
        const target = mode === 1 ? grid : list
        if (target.count > 0) moveBy(target, delta, 0)
    }

    // The row whose name is being edited, or -1.
    property int renamingRow: -1
    signal notice(string message)
    /// A file was double-clicked or Entered: the window opens it.
    signal fileActivated(string path)
    /// A right click on an item in a column other than the current folder's.
    signal pathMenuRequested(string path, bool isFolder)

    function startRename() {
        const target = mode === 1 ? grid : list
        if (folderModel.selectionCount === 1 && target.currentIndex >= 0 && folderModel.isSelected(target.currentIndex))
            renamingRow = target.currentIndex
    }
    // Starts a drag of the selection (or `row`) from `item`, carrying file URIs.
    function startDrag(item, row) {
        item.Drag.mimeData = { "text/uri-list": transfers.dragUris(folderModel, row) }
        item.Drag.active = true
    }
    function finishRename(name) {
        const row = renamingRow
        renamingRow = -1
        if (name.length > 0) folderModel.rename(row, name)
        focusList()
    }
    Connections {
        target: view.folderModel
        function onFailed(message) { view.notice(message) }
        function onFolderChanged() { view.renamingRow = -1; view.pendingRename = "" }
    }
    // A new folder or file is renamed as soon as it shows up.
    property string pendingRename
    Connections {
        target: view.ops
        function onCreated(folder, name) {
            if (folder === view.folderModel) {
                view.pendingRename = name
                renameWait.restart()
            }
        }
    }
    Timer {
        id: renameWait
        interval: 50
        repeat: true
        property int tries: 0
        onTriggered: {
            const row = view.folderModel.rowOfName(view.pendingRename)
            if (row >= 0) {
                stop()
                tries = 0
                view.pendingRename = ""
                view.folderModel.select(row)
                const target = view.mode === 1 ? grid : list
                target.currentIndex = row
                target.positionViewAtIndex(row, view.mode === 1 ? GridView.Contain : ListView.Contain)
                view.renamingRow = row
            } else if (++tries > 60) {
                stop()
                tries = 0
            }
        }
    }
    function requestContext(row) {
        if (row >= 0 && !folderModel.isSelected(row)) folderModel.select(row)
        contextRequested(row)
    }

    // List columns that give way as the view narrows.
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    function focusList() {
        (mode === 1 ? grid : mode === 2 ? columnsView : list).forceActiveFocus()
    }
    // A click on a row: it selects, and opens what Settings says opens with one click (files or
    // folders, in each view). A double click opens the rest.
    function rowClicked(model, row, modifiers) {
        click(model, row, modifiers)
        if (modifiers & (Qt.ControlModifier | Qt.ShiftModifier)) return
        if (AppSettings.clicks(mode, !model.isFolder(row)) === 1) open(model, row)
    }
    function rowDoubleClicked(model, row) {
        if (AppSettings.clicks(mode, !model.isFolder(row)) === 2) open(model, row)
    }
    function open(model, row) {
        if (!model.isFolder(row)) fileActivated(model.pathAt(row))
        else if (mode === 2) browser.goColumnRow(model, row)
        else browser.goRow(row)
    }
    // What a click does to the selection.
    function click(model, row, modifiers) {
        if (modifiers & Qt.ControlModifier) model.toggle(row)
        else if (modifiers & Qt.ShiftModifier) model.extendTo(row)
        else model.select(row)
    }
    // Moves the keyboard's place by `step` rows, selecting there (Shift extends).
    function moveBy(target, step, modifiers) {
        const next = Math.max(0, Math.min(target.count - 1, target.currentIndex + step))
        target.currentIndex = next
        if (modifiers & Qt.ShiftModifier) folderModel.extendTo(next)
        else folderModel.select(next)
        target.positionViewAtIndex(next, target === grid ? GridView.Contain : ListView.Contain)
    }
    function openCurrent(target) {
        activate(target.currentIndex)
    }
    function activate(row) {
        if (row < 0) return
        if (folderModel.isFolder(row)) browser.goRow(row)
        else fileActivated(folderModel.pathAt(row))
    }

    Keys.onPressed: (event) => {
        // Typing a name filters the folder to what matches it.
        if (mode !== 2 && event.text.length === 1 && event.text >= " " && event.text !== " " &&
                !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) {
            filterField.text = event.text
            filterField.forceActiveFocus()
            folderModel.filter = event.text
            event.accepted = true
            return
        }
        if (event.matches(StandardKey.SelectAll)) { folderModel.selectAll(); event.accepted = true }
        else if (event.key === Qt.Key_Escape) { folderModel.clearSelection(); event.accepted = true }
        else if (event.key === Qt.Key_F2 && mode !== 2) { startRename(); event.accepted = true }
    }

    // A right click on empty space.
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: view.requestContext(-1)
    }

    // A drop anywhere else goes into the folder shown.
    DropArea {
        anchors.fill: parent
        enabled: view.mode !== 2
        onDropped: (drop) => { if (view.transfers.dropHere(drop.urls, view.folderModel)) drop.accept() }
    }

    // A notice above the folder when an interrupted operation left originals here.
    Rectangle {
        id: leftoversNotice
        visible: view.folderModel !== null && view.folderModel.leftovers.length > 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 12
        height: visible ? 40 : 0
        radius: Theme.radius
        color: Qt.rgba(Theme.warning.r, Theme.warning.g, Theme.warning.b, 0.10)
        border.width: 1
        border.color: Qt.rgba(Theme.warning.r, Theme.warning.g, Theme.warning.b, 0.35)
        z: 2
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 6
            Icon { path: "M8 2.5 14 13H2zM8 6.5v3M8 11.5h.01"; color: Theme.warning }
            Text {
                Layout.fillWidth: true
                text: qsTr("Items left by an interrupted operation")
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
            Button { text: qsTr("Review…"); onClicked: leftoversDialog.review() }
        }
    }
    LeftoversDialog {
        id: leftoversDialog
        ops: view.ops
        folderModel: view.folderModel
        listening: view.Window.window === null || view.Window.window.foremost !== false
    }

    // The filter typed over the folder.
    Rectangle {
        id: filterBar
        visible: view.folderModel !== null && view.folderModel.filter.length > 0
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 10
        anchors.topMargin: leftoversNotice.visible ? 56 : 4
        width: 280
        height: 34
        radius: Theme.radius
        color: Theme.field
        border.width: 1
        border.color: Theme.selectionEdge
        z: 3
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 4
            spacing: 6
            Icon { path: "M2.5 3.5h11l-4.5 5v4l-2 1v-5z"; color: Theme.textDim; size: 14 }
            TextField {
                id: filterField
                objectName: "filterField"
                Layout.fillWidth: true
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
                placeholderText: qsTr("Filter")
                placeholderTextColor: Theme.textMute
                background: null
                onTextEdited: view.folderModel.filter = text
                Keys.onEscapePressed: { view.folderModel.filter = ""; view.focusList() }
                Keys.onDownPressed: view.focusList()
                Keys.onReturnPressed: view.focusList()
            }
            IconButton {
                width: 26; height: 26
                label: qsTr("Clear filter")
                iconPath: "m4.5 4.5 7 7M11.5 4.5l-7 7"
                onClicked: { view.folderModel.filter = ""; view.focusList() }
            }
        }
    }

    // List
    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: leftoversNotice.visible ? 52 : 0
        visible: view.mode === 0
        spacing: 0

        RowLayout {
            Layout.preferredHeight: 36
            Layout.leftMargin: 24
            spacing: 0
            Repeater {
                // The name takes what the other columns leave, up to 600 px.
                model: [[qsTr("Name"), -1, false, "name"], [qsTr("Mode"), 70, false, ""], [qsTr("Size"), 91, true, "size"],
                        ["", 40, false, ""], [qsTr("Kind"), 119, false, ""], [qsTr("Modified"), 140, false, "modified"]]
                Item {
                    id: heading
                    required property var modelData
                    required property int index
                    readonly property bool sorted: view.folderModel !== null && modelData[3] === view.folderModel.sortKey
                    visible: index === 1 ? view.showMode : index === 4 ? view.showKind : true
                    Layout.fillWidth: modelData[1] < 0
                    Layout.maximumWidth: modelData[1] < 0 ? 600 : index === 3 && !view.showGap ? 16 : modelData[1]
                    Layout.preferredWidth: modelData[1] < 0 ? 600 : index === 3 && !view.showGap ? 16 : modelData[1]
                    implicitHeight: 24
                    RowLayout {
                        anchors.fill: parent
                        spacing: 6
                        layoutDirection: heading.modelData[2] ? Qt.RightToLeft : Qt.LeftToRight
                        Text {
                            text: heading.modelData[0]
                            color: heading.sorted ? Theme.textDim : Theme.textMute
                            font.family: Theme.family
                            font.pointSize: Theme.tiny
                            font.weight: Font.DemiBold
                        }
                        Icon {
                            visible: heading.sorted
                            path: view.folderModel && view.folderModel.sortDescending ? "m4 6 4 4 4-4" : "m4 10 4-4 4 4"
                            color: Theme.accent
                            size: 10
                            strokeWidth: 2
                        }
                        Item { Layout.fillWidth: true }
                    }
                    TapHandler {
                        enabled: heading.modelData[3].length > 0
                        onTapped: view.browser.sortBy(heading.modelData[3])
                    }
                    HoverHandler { enabled: heading.modelData[3].length > 0; cursorShape: Qt.PointingHandCursor }
                }
            }
            Item { Layout.fillWidth: true }
        }
        ListView {
            id: list
            objectName: "fileList"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            clip: true
            reuseItems: true
            cacheBuffer: 600
            focus: view.mode === 0
            interactive: false
            model: view.folderModel
            currentIndex: -1
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            delegate: FileRow {
                width: ListView.view.width
                showGap: view.showGap
                showMode: view.showMode
                showKind: view.showKind
                id: rowItem
                editing: index === view.renamingRow
                Drag.dragType: Drag.Automatic
                Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
                DragHandler {
                    target: null
                    enabled: !rowItem.editing
                    onActiveChanged: active ? view.startDrag(rowItem, rowItem.index) : (rowItem.Drag.active = false)
                }
                DropArea {
                    anchors.fill: parent
                    enabled: rowItem.isFolder
                    onDropped: (drop) => { if (view.transfers.dropInto(drop.urls, view.folderModel, rowItem.index)) drop.accept() }
                    Rectangle { anchors.fill: parent; radius: Theme.radius; color: "transparent"; border.width: parent.containsDrag ? 1 : 0; border.color: Theme.accent }
                }
                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: { list.currentIndex = rowItem.index; view.requestContext(rowItem.index) }
                }
                onRenameAccepted: (name) => view.finishRename(name)
                onRenameCancelled: { view.renamingRow = -1; view.focusList() }
                onClicked: (modifiers) => { list.currentIndex = index; view.rowClicked(view.folderModel, index, modifiers); list.forceActiveFocus() }
                onActivated: view.rowDoubleClicked(view.folderModel, index)
            }
            Keys.onUpPressed: (event) => view.moveBy(list, -1, event.modifiers)
            Keys.onDownPressed: (event) => view.moveBy(list, 1, event.modifiers)
            Keys.onReturnPressed: view.openCurrent(list)
            Keys.onEnterPressed: view.openCurrent(list)
            RubberBand {
                anchors.fill: parent
                view: list
                rowAt: (x, y) => list.indexAt(x, y)
                rowsIn: (rect) => {
                    const first = Math.max(0, Math.floor(rect.y / Theme.rowHeight))
                    const last = Math.min(list.count - 1, Math.floor((rect.y + rect.height) / Theme.rowHeight))
                    const rows = []
                    for (let row = first; row <= last; ++row) rows.push(row)
                    return rows
                }
                onSelected: (rows, add) => view.folderModel.selectRows(rows, add)
                onCleared: view.folderModel.clearSelection()
            }
            Text {
                anchors.centerIn: parent
                visible: view.folderModel !== null && (view.folderModel.error.length > 0 || (!view.folderModel.loading && list.count === 0))
                text: view.folderModel !== null && view.folderModel.error.length > 0 ? view.folderModel.error : qsTr("This folder is empty")
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.body
            }
        }
    }

    // Icons
    GridView {
        id: grid
        objectName: "fileGrid"
        anchors.fill: parent
        anchors.margins: 12
        visible: view.mode === 1
        clip: true
        focus: view.mode === 1
        interactive: false
        // Only a shown grid holds the model, so a hidden view asks for no rows.
        model: visible ? view.folderModel : null
        cellWidth: 120
        cellHeight: 124
        currentIndex: -1
        ScrollBar.vertical: ScrollBar {}
        readonly property int perRow: Math.max(1, Math.floor(width / cellWidth))
        delegate: FileTile {
            width: GridView.view.cellWidth - 8
            height: GridView.view.cellHeight - 8
            id: tileItem
            editing: index === view.renamingRow
            Drag.dragType: Drag.Automatic
            Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
            DragHandler {
                target: null
                enabled: !tileItem.editing
                onActiveChanged: active ? view.startDrag(tileItem, tileItem.index) : (tileItem.Drag.active = false)
            }
            DropArea {
                anchors.fill: parent
                enabled: tileItem.isFolder
                onDropped: (drop) => { if (view.transfers.dropInto(drop.urls, view.folderModel, tileItem.index)) drop.accept() }
                Rectangle { anchors.fill: parent; radius: 11; color: "transparent"; border.width: parent.containsDrag ? 1 : 0; border.color: Theme.accent }
            }
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: { grid.currentIndex = tileItem.index; view.requestContext(tileItem.index) }
            }
            onRenameAccepted: (name) => view.finishRename(name)
            onRenameCancelled: { view.renamingRow = -1; view.focusList() }
            onClicked: (modifiers) => { grid.currentIndex = index; view.rowClicked(view.folderModel, index, modifiers); grid.forceActiveFocus() }
            onActivated: view.rowDoubleClicked(view.folderModel, index)
        }
        Keys.onLeftPressed: (event) => view.moveBy(grid, -1, event.modifiers)
        Keys.onRightPressed: (event) => view.moveBy(grid, 1, event.modifiers)
        Keys.onUpPressed: (event) => view.moveBy(grid, -grid.perRow, event.modifiers)
        Keys.onDownPressed: (event) => view.moveBy(grid, grid.perRow, event.modifiers)
        Keys.onReturnPressed: view.openCurrent(grid)
        Keys.onEnterPressed: view.openCurrent(grid)
        RubberBand {
            anchors.fill: parent
            view: grid
            rowAt: (x, y) => grid.indexAt(x, y)
            rowsIn: (rect) => {
                const rows = []
                const firstRow = Math.max(0, Math.floor(rect.y / grid.cellHeight))
                const lastRow = Math.floor((rect.y + rect.height) / grid.cellHeight)
                const firstColumn = Math.max(0, Math.floor(rect.x / grid.cellWidth))
                const lastColumn = Math.min(grid.perRow - 1, Math.floor((rect.x + rect.width) / grid.cellWidth))
                for (let r = firstRow; r <= lastRow; ++r)
                    for (let c = firstColumn; c <= lastColumn; ++c) {
                        const at = r * grid.perRow + c
                        if (at < grid.count) rows.push(at)
                    }
                return rows
            }
            onSelected: (rows, add) => view.folderModel.selectRows(rows, add)
            onCleared: view.folderModel.clearSelection()
        }
    }

    // Columns
    ListView {
        id: columnsView
        objectName: "columns"
        anchors.fill: parent
        anchors.topMargin: 6
        visible: view.mode === 2
        orientation: ListView.Horizontal
        clip: true
        focus: view.mode === 2
        boundsBehavior: Flickable.StopAtBounds
        model: visible && view.browser ? view.browser.columns : null
        ScrollBar.horizontal: ScrollBar {}
        // The newest column stays in view.
        onCountChanged: Qt.callLater(() => positionViewAtEnd())
        delegate: FolderColumn {
            height: ListView.view.height
            last: index === ListView.view.count - 1
            onOpenRow: (row) => view.browser.goColumnRow(folderModel, row)
            onClickRow: (row, modifiers) => view.rowClicked(folderModel, row, modifiers)
            onDoubleClickRow: (row) => view.rowDoubleClicked(folderModel, row)
            onMenuRow: (row) => {
                if (folderModel === view.folderModel) {
                    view.requestContext(row)
                } else {
                    view.pathMenuRequested(folderModel.pathAt(row), folderModel.isFolder(row))
                }
            }
        }
    }
}
