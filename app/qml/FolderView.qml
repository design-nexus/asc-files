// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// A folder, in the view the tab has chosen for it: List (columns you can sort by), Icons,
// Columns, Compact List or Gallery (Dual Pane is two of these). Clicks select (Ctrl adds or
// removes, Shift extends), a box dragged from empty space selects what it covers, and a double
// click or Enter opens a folder. A focus scope: in Dual Pane each pane keeps its own focus.
FocusScope {
    id: view

    required property var browser
    required property var transfers
    required property var ops
    /// A right click: on `row` (selected now if it was not), or on empty space (-1).
    signal contextRequested(int row)
    readonly property var folderModel: browser ? browser.folderModel : null
    /// The view: the tab's, or one a Dual Pane pane is held to (-1: the tab's).
    property int forcedMode: -1
    readonly property int mode: forcedMode >= 0 ? forcedMode : browser ? browser.view : 0
    /// The window's preview, which Gallery shows large.
    property var preview: null
    /// The list the keyboard and selection go by in this view.
    readonly property var current: mode === 1 ? grid : mode === 3 ? compact : mode === 4 ? strip : list
    readonly property int selectionCount: folderModel ? folderModel.selectionCount : 0
    readonly property string status: {
        const items = qsTr("%n items", "", list.count)
        return selectionCount > 0 ? items + " · " + qsTr("%n selected", "", selectionCount) : items
    }
    // The item the keyboard is on, for the preview; read again as the folder or selection changes.
    readonly property string currentPath: folderModel ? (folderModel.generation, folderModel.selectionCount,
        folderModel.pathAt(current.currentIndex)) : ""
    // Moves to the next or previous item (`delta` rows), as the preview's arrows do.
    function step(delta) {
        if (current.count > 0) moveBy(current, delta, 0)
    }

    // The row whose name is being edited, or -1.
    property int renamingRow: -1
    signal notice(string message)
    /// A file was double-clicked or Entered: the window opens it.
    signal fileActivated(string path)
    /// A drop was taken: what it did (Transfers.Dropped), into `target`.
    signal dropped(int result, string target)
    /// A right click on an item in a column other than the current folder's.
    signal pathMenuRequested(string path, bool isFolder)

    function startRename() {
        if (folderModel.selectionCount === 1 && current.currentIndex >= 0 && folderModel.isSelected(current.currentIndex))
            renamingRow = current.currentIndex
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
        // An archive made or extracted here is selected once it shows up.
        function onArchived(path) {
            const slash = path.lastIndexOf("/")
            if (view.folderModel && path.substring(0, slash) === view.folderModel.folder) {
                view.pendingSelect = path.substring(slash + 1)
                selectWait.restart()
            }
        }
    }
    property string pendingSelect
    Timer {
        id: selectWait
        interval: 50
        repeat: true
        property int tries: 0
        onTriggered: {
            const row = view.folderModel.rowOfName(view.pendingSelect)
            if (row >= 0 || ++tries > 60) {
                stop()
                tries = 0
                if (row >= 0) {
                    view.folderModel.clearSelection()
                    view.folderModel.select(row)
                    view.current.currentIndex = row
                    view.current.positionViewAtIndex(row, ListView.Contain)
                }
                view.pendingSelect = ""
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
                view.current.currentIndex = row
                view.current.positionViewAtIndex(row, ListView.Contain)
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

    // List view's columns after Name: Settings' order and widths, a copy while one is dragged.
    property var columns: AppSettings.listColumns
    Connections {
        target: AppSettings
        function onChanged() { view.columns = Qt.binding(() => AppSettings.listColumns) }
    }
    function sizeColumn(index, width, keep) {
        const copy = columns.map(column => Object.assign({}, column))
        copy[index].width = Math.max(40, Math.min(400, Math.round(width)))
        columns = copy
        if (keep) AppSettings.listColumns = copy
    }
    // Moves the column at `from` to where `middle` (an x in the header) falls among the others.
    function moveColumn(from, middle) {
        let to = 0
        for (let i = 0; i < headings.count; ++i) {
            const other = headings.itemAt(i)
            if (i !== from && other.visible && other.x + other.width / 2 < middle) ++to
        }
        if (to === from) return
        const copy = columns.slice()
        const [moved] = copy.splice(from, 1)
        copy.splice(to, 0, moved)
        AppSettings.listColumns = copy
    }

    // List columns that give way as the view narrows.
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    // A view chosen takes the keyboard. A pane whose view is fixed (Dual Pane's second) chooses
    // none: showing it must not take the keyboard from the pane in use.
    onModeChanged: if (forcedMode < 0) Qt.callLater(focusList)
    function focusList() {
        (mode === 2 ? columnsView : current).forceActiveFocus()
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
        target.positionViewAtIndex(next, ListView.Contain)
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
        if (AppSettings.typeToFilter && mode !== 2 && event.text.length === 1 && event.text >= " " && event.text !== " " &&
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
        // The menu key or Shift+F10: the current item's menu, or the folder's when there is none.
        else if (event.key === Qt.Key_Menu || (event.key === Qt.Key_F10 && (event.modifiers & Qt.ShiftModifier))) {
            const row = view.current && view.current.currentIndex >= 0 && view.current.currentIndex < view.current.count
                      ? view.current.currentIndex : -1
            requestContext(row)
            event.accepted = true
        }
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
        onDropped: (drop) => {
            const result = view.transfers.dropHere(drop.urls, view.folderModel)
            if (result !== Transfers.Refused) { drop.accept(); view.dropped(result, view.browser.location) }
        }
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
            // Name, which takes what the other columns leave (up to 600 px), then the others, in
            // the order and widths chosen: drag a heading to move it, its edge to size it.
            Heading {
                title: qsTr("Name")
                sortKey: "name"
                Layout.fillWidth: true
                Layout.maximumWidth: 600
                Layout.preferredWidth: 600
            }
            Repeater {
                id: headings
                model: view.columns
                Heading {
                    id: heading
                    required property var modelData
                    required property int index
                    readonly property string key: modelData.key
                    readonly property int gap: key === "size" ? (view.showGap ? 40 : 16) : 0
                    title: key === "mode" ? qsTr("Mode") : key === "size" ? qsTr("Size") : key === "kind" ? qsTr("Kind") : qsTr("Modified")
                    sortKey: key === "size" ? "size" : key === "kind" ? "type" : key === "modified" ? "modified" : ""
                    rightAligned: key === "size"
                    trailing: gap
                    visible: (key !== "mode" || view.showMode) && (key !== "kind" || view.showKind)
                    Layout.preferredWidth: modelData.width + gap
                    Layout.maximumWidth: modelData.width + gap
                    // Moved: dropped where its middle is among the others.
                    z: move.active ? 2 : 0
                    transform: Translate { x: move.active ? move.translation.x : 0 }
                    DragHandler {
                        id: move
                        target: null
                        xAxis.enabled: true
                        yAxis.enabled: false
                        onActiveChanged: if (!active) view.moveColumn(heading.index, heading.x + heading.width / 2 + translation.x)
                    }
                    // Its right edge sizes it; a double click puts its width back.
                    Item {
                        anchors.right: parent.right
                        anchors.rightMargin: heading.gap - 3
                        anchors.top: parent.top
                        anchors.bottom: parent.bottom
                        width: 8
                        HoverHandler { id: edgeHover; cursorShape: Qt.SplitHCursor }
                        Rectangle { anchors.centerIn: parent; width: 2; height: 14; radius: 1; color: Theme.textMute; visible: edgeHover.hovered || sizing.active }
                        property int startWidth
                        DragHandler {
                            id: sizing
                            target: null
                            yAxis.enabled: false
                            onActiveChanged: {
                                if (active) parent.startWidth = heading.modelData.width
                                else AppSettings.listColumns = view.columns
                            }
                            onTranslationChanged: if (active) view.sizeColumn(heading.index, parent.startWidth + translation.x)
                        }
                        TapHandler {
                            onDoubleTapped: view.sizeColumn(heading.index, AppSettings.defaultColumnWidth(heading.key), true)
                        }
                    }
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
                columns: view.columns
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
                    onDropped: (drop) => {
                        const result = view.transfers.dropInto(drop.urls, view.folderModel, rowItem.index)
                        if (result !== Transfers.Refused) { drop.accept(); view.dropped(result, view.folderModel.pathAt(rowItem.index)) }
                    }
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
        // The icon size is the folder's own (Ctrl+scroll, Ctrl+= and Ctrl+-).
        readonly property int iconSize: view.browser ? view.browser.iconSize : 64
        cellWidth: iconSize + 56
        cellHeight: iconSize + 60
        currentIndex: -1
        ScrollBar.vertical: ScrollBar {}
        readonly property int perRow: Math.max(1, Math.floor(width / cellWidth))
        delegate: FileTile {
            width: GridView.view.cellWidth - 8
            height: GridView.view.cellHeight - 8
            iconSize: grid.iconSize
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
                onDropped: (drop) => {
                    const result = view.transfers.dropInto(drop.urls, view.folderModel, tileItem.index)
                    if (result !== Transfers.Refused) { drop.accept(); view.dropped(result, view.folderModel.pathAt(tileItem.index)) }
                }
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
        WheelHandler {
            acceptedModifiers: Qt.ControlModifier
            onWheel: (event) => view.browser.zoom(event.angleDelta.y > 0 ? 1 : -1)
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

    // A List column's heading: a click sorts by it (when it sorts), again turns it around.
    component Heading: Item {
        id: heading
        property string title
        property string sortKey
        property bool rightAligned: false
        /// Room kept after it (Size's gap).
        property int trailing: 0
        readonly property bool sorted: view.folderModel !== null && sortKey.length > 0 && sortKey === view.folderModel.sortKey
        implicitHeight: 24
        RowLayout {
            anchors.fill: parent
            anchors.rightMargin: heading.trailing
            spacing: 6
            layoutDirection: heading.rightAligned ? Qt.RightToLeft : Qt.LeftToRight
            Text {
                text: heading.title
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
            enabled: heading.sortKey.length > 0
            onTapped: view.browser.sortBy(heading.sortKey)
        }
        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: columnsMenu.popup()
        }
    }
    ContextMenu {
        id: columnsMenu
        MenuItem { text: qsTr("Reset Columns"); onTriggered: AppSettings.resetListColumns() }
    }

    // Compact List: small icons and names in columns that fill top to bottom, scrolled sideways.
    GridView {
        id: compact
        objectName: "fileCompact"
        anchors.fill: parent
        anchors.margins: 12
        visible: view.mode === 3
        clip: true
        focus: view.mode === 3
        flow: GridView.FlowTopToBottom
        boundsBehavior: Flickable.StopAtBounds
        model: visible ? view.folderModel : null
        cellWidth: 260
        cellHeight: Theme.rowHeight - 4
        currentIndex: -1
        ScrollBar.horizontal: ScrollBar {}
        readonly property int perColumn: Math.max(1, Math.floor(height / cellHeight))
        delegate: Rectangle {
            id: compactItem
            required property int index
            required property string name
            required property bool isFolder
            required property bool loaded
            required property bool selected
            required property string customColor
            required property string customIcon
            readonly property bool editing: index === view.renamingRow
            width: GridView.view.cellWidth - 6
            height: GridView.view.cellHeight - 2
            radius: Theme.radius
            color: selected ? Theme.selection : compactHover.hovered ? Theme.hover : "transparent"
            Drag.dragType: Drag.Automatic
            Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
            HoverHandler { id: compactHover }
            DragHandler {
                target: null
                enabled: !compactItem.editing
                onActiveChanged: active ? view.startDrag(compactItem, compactItem.index) : (compactItem.Drag.active = false)
            }
            DropArea {
                anchors.fill: parent
                enabled: compactItem.isFolder
                onDropped: (drop) => {
                    const result = view.transfers.dropInto(drop.urls, view.folderModel, compactItem.index)
                    if (result !== Transfers.Refused) { drop.accept(); view.dropped(result, view.folderModel.pathAt(compactItem.index)) }
                }
                Rectangle { anchors.fill: parent; radius: Theme.radius; color: "transparent"; border.width: parent.containsDrag ? 1 : 0; border.color: Theme.accent }
            }
            TapHandler {
                onTapped: (point) => { compact.currentIndex = compactItem.index; view.rowClicked(view.folderModel, compactItem.index, point.modifiers); compact.forceActiveFocus() }
                onDoubleTapped: view.rowDoubleClicked(view.folderModel, compactItem.index)
            }
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: { compact.currentIndex = compactItem.index; view.requestContext(compactItem.index) }
            }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                spacing: 8
                Icon {
                    visible: !compactItem.customIcon.startsWith("emoji:")
                    size: 14
                    color: compactItem.customColor.length > 0 ? compactItem.customColor : compactItem.isFolder ? Theme.accent : Theme.folderIcon
                    path: compactItem.customIcon.length > 0 ? compactItem.customIcon
                        : compactItem.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
                Text {
                    visible: compactItem.customIcon.startsWith("emoji:")
                    text: compactItem.customIcon.substring(6)
                    font.pixelSize: 12
                }
                Text {
                    visible: !compactItem.editing
                    Layout.fillWidth: true
                    text: compactItem.loaded ? compactItem.name : ""
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.body
                }
                NameEditor {
                    visible: compactItem.editing
                    Layout.fillWidth: true
                    Layout.preferredHeight: 24
                    original: compactItem.name
                    isFolder: compactItem.isFolder
                    onVisibleChanged: if (visible) begin()
                    onCommitted: (name) => view.finishRename(name)
                    onCancelled: { view.renamingRow = -1; view.focusList() }
                }
            }
        }
        Keys.onUpPressed: (event) => view.moveBy(compact, -1, event.modifiers)
        Keys.onDownPressed: (event) => view.moveBy(compact, 1, event.modifiers)
        Keys.onLeftPressed: (event) => view.moveBy(compact, -compact.perColumn, event.modifiers)
        Keys.onRightPressed: (event) => view.moveBy(compact, compact.perColumn, event.modifiers)
        Keys.onReturnPressed: view.openCurrent(compact)
        Keys.onEnterPressed: view.openCurrent(compact)
    }

    // Gallery: the item the keyboard is on, large, over a strip of the folder's items.
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        visible: view.mode === 4
        spacing: 10
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.radius + 4
            color: Theme.window
            border.width: 1
            border.color: Theme.panelEdge
            clip: true
            Loader {
                anchors.fill: parent
                active: view.mode === 4 && view.preview !== null
                sourceComponent: PreviewContent { preview: view.preview; closable: false }
            }
        }
        ListView {
            id: strip
            objectName: "fileStrip"
            Layout.fillWidth: true
            Layout.preferredHeight: 116
            orientation: ListView.Horizontal
            clip: true
            spacing: 6
            focus: view.mode === 4
            boundsBehavior: Flickable.StopAtBounds
            model: view.mode === 4 ? view.folderModel : null
            currentIndex: -1
            ScrollBar.horizontal: ScrollBar {}
            delegate: FileTile {
                id: stripItem
                width: 104
                height: 104
                iconSize: 48
                editing: index === view.renamingRow
                Drag.dragType: Drag.Automatic
                Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
                DragHandler {
                    target: null
                    enabled: !stripItem.editing
                    onActiveChanged: active ? view.startDrag(stripItem, stripItem.index) : (stripItem.Drag.active = false)
                }
                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: { strip.currentIndex = stripItem.index; view.requestContext(stripItem.index) }
                }
                onRenameAccepted: (name) => view.finishRename(name)
                onRenameCancelled: { view.renamingRow = -1; view.focusList() }
                onClicked: (modifiers) => { strip.currentIndex = index; view.rowClicked(view.folderModel, index, modifiers); strip.forceActiveFocus() }
                onActivated: view.rowDoubleClicked(view.folderModel, index)
            }
            Keys.onLeftPressed: (event) => view.moveBy(strip, -1, event.modifiers)
            Keys.onRightPressed: (event) => view.moveBy(strip, 1, event.modifiers)
            Keys.onReturnPressed: view.openCurrent(strip)
            Keys.onEnterPressed: view.openCurrent(strip)
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
