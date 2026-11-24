// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A folder, in the view the tab has chosen for it: List (columns you can sort by), Icons, or
// Columns. Clicks select (Ctrl adds or removes, Shift extends), a box dragged from empty space
// selects what it covers, and a double click or Enter opens a folder.
Item {
    id: view

    required property var browser
    required property var transfers
    readonly property var folderModel: browser ? browser.folderModel : null
    readonly property int mode: browser ? browser.view : 0
    readonly property int selectionCount: folderModel ? folderModel.selectionCount : 0
    readonly property string status: {
        const items = qsTr("%n items", "", list.count)
        return selectionCount > 0 ? items + " · " + qsTr("%n selected", "", selectionCount) : items
    }
    // The row whose name is being edited, or -1.
    property int renamingRow: -1
    signal notice(string message)

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
        function onFolderChanged() { view.renamingRow = -1 }
    }

    // List columns that give way as the view narrows.
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    function focusList() {
        (mode === 1 ? grid : mode === 2 ? columnsView : list).forceActiveFocus()
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
        if (target.currentIndex >= 0 && folderModel.isFolder(target.currentIndex))
            browser.goRow(target.currentIndex)
    }

    Keys.onPressed: (event) => {
        if (event.matches(StandardKey.SelectAll)) { folderModel.selectAll(); event.accepted = true }
        else if (event.key === Qt.Key_Escape) { folderModel.clearSelection(); event.accepted = true }
        else if (event.key === Qt.Key_F2 && mode !== 2) { startRename(); event.accepted = true }
    }

    // A drop anywhere else goes into the folder shown.
    DropArea {
        anchors.fill: parent
        enabled: view.mode !== 2
        onDropped: (drop) => { if (view.transfers.dropHere(drop.urls, view.folderModel)) drop.accept() }
    }

    // List
    ColumnLayout {
        anchors.fill: parent
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
                onRenameAccepted: (name) => view.finishRename(name)
                onRenameCancelled: { view.renamingRow = -1; view.focusList() }
                onClicked: (modifiers) => { list.currentIndex = index; view.click(view.folderModel, index, modifiers); list.forceActiveFocus() }
                onActivated: if (isFolder) view.browser.goRow(index)
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
            onRenameAccepted: (name) => view.finishRename(name)
            onRenameCancelled: { view.renamingRow = -1; view.focusList() }
            onClicked: (modifiers) => { grid.currentIndex = index; view.click(view.folderModel, index, modifiers); grid.forceActiveFocus() }
            onActivated: if (isFolder) view.browser.goRow(index)
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
            onClickRow: (row, modifiers) => view.click(folderModel, row, modifiers)
        }
    }
}
