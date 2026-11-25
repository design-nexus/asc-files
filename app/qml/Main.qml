// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window

import AscFiles
import AscFiles.System

// The Files window in its current layout: header across the top, the sidebar of places, and the
// rounded content panel holding the tab row and the folder.
ApplicationWindow {
    id: window

    required property var tabs
    required property var places
    required property var recent
    required property var favorites
    required property var trash
    required property var downloads
    required property var jobModel
    required property var transfers
    required property var ops
    required property var preview
    required property var search
    required property var backend
    /// The app's windows (absent in a test of one window).
    property var windows: null
    /// Whether operations answer here: the window used last does (their notices, clashes and
    /// questions show in one window only).
    readonly property bool foremost: windows === null || windows.current === window

    readonly property var browser: tabs.current
    readonly property var folderModel: browser ? browser.folderModel : null

    width: 1600
    height: 968
    visible: true
    color: Theme.window
    title: browser ? browser.title : ""

    // The view the current tab shows, by its page (Browser.Folder, Overview, Recent, Favorites,
    // Trash, in that order).
    readonly property int page: browser ? browser.page : 0
    readonly property var view: pages.children[page]
    // The preview drawer: shown as a Popup over the window (as legacy's default) or a Panel beside
    // the folder, and following the item the keyboard is on.
    property bool previewOpen: false
    readonly property bool previewAsPanel: AppSettings.previewStyle === "panel"
    readonly property string previewPath: previewOpen && view ? view.currentPath : ""
    onPreviewPathChanged: preview.show(previewPath)

    // The sidebar as chosen, railed when the window is too narrow for it in full.
    readonly property int sidebarMode: places.mode === 2 ? 2 : places.mode === 0 && width < 900 ? 1 : places.mode

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Header
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: Theme.headerHeight
            Layout.leftMargin: 16
            Layout.rightMargin: 10
            spacing: 4

            RowLayout {
                Layout.preferredWidth: Theme.sidebarWidth - 16
                Layout.minimumWidth: implicitWidth
                spacing: 4
                Icon { path: "M2 4.2c0-.6.5-1.2 1.2-1.2h3.3l1.5 1.5h4.8c.7 0 1.2.5 1.2 1.2v6.1c0 .7-.5 1.2-1.2 1.2H3.2c-.7 0-1.2-.5-1.2-1.2z"; color: Theme.accent; size: 20 }
                Text { visible: window.width >= 1000; text: qsTr("Files"); color: Theme.accentText; font.family: Theme.family; font.pointSize: Theme.title; font.weight: Font.DemiBold; Layout.leftMargin: 6; Layout.rightMargin: 10 }
                IconButton { id: sidebarToggle; label: qsTr("Sidebar (Ctrl+B)"); checked: window.places.mode !== 2; onClicked: window.places.stepMode(); iconPath: "M4 3h8a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 12 13H4a1.5 1.5 0 0 1-1.5-1.5v-7A1.5 1.5 0 0 1 4 3zM6.5 3v10" }
                IconButton { label: qsTr("Back (Alt+Left)"); enabled: window.browser && window.browser.canGoBack; iconPath: "M13 8H3M7 4 3 8l4 4"; onClicked: window.browser.back() }
                IconButton { label: qsTr("Forward (Alt+Right)"); enabled: window.browser && window.browser.canGoForward; iconPath: "M3 8h10M9 4l4 4-4 4"; onClicked: window.browser.forward() }
                IconButton { label: qsTr("Up (Alt+Up)"); enabled: window.browser && window.browser.canGoUp; iconPath: "M8 13V3M4 7l4-4 4 4"; onClicked: window.browser.up() }
            }
            Breadcrumb {
                id: breadcrumb
                browser: window.browser
                onMenuRequested: (path) => pathMenu.show(path, true, true)
                Layout.leftMargin: window.width >= 1000 ? 22 : 6
                Layout.fillWidth: true
                Layout.minimumWidth: 60
                Layout.maximumWidth: 520
                onEdited: window.view.focusList()
            }
            Item { Layout.fillWidth: true }
            SearchField {
                id: searchField
                objectName: "searchField"
                Layout.fillWidth: true
                Layout.maximumWidth: 352
                Layout.minimumWidth: 120
                placeholderText: qsTr("Search Home and drives")
                // Typing searches, in the Search page of this tab; the folder it began in can
                // narrow it.
                onTextEdited: {
                    if (text.length === 0) return
                    if (window.page !== 5) {
                        searchPage.origin = window.page === 0 ? window.browser.location : ""
                        window.browser.goPage(5)
                    }
                    window.search.search(text, window.search.roots.length > 0 && window.search.query.length > 0
                                               ? window.search.roots : window.places.searchRoots())
                }
                Keys.onReturnPressed: window.search.searchNow()
                Keys.onDownPressed: window.view.focusList()
                Keys.onEscapePressed: { text = ""; window.view.focusList() }
            }
            Item { Layout.fillWidth: true }
            Rectangle {
                color: Theme.field; radius: 9
                implicitWidth: views.implicitWidth + 4; implicitHeight: 34
                Row {
                    id: views
                    anchors.centerIn: parent
                    spacing: 2
                    IconButton { width: 30; height: 30; label: qsTr("List (Ctrl+1)"); checked: window.browser !== null && window.page === 0 && window.browser.view === 0; onClicked: window.browser.setView(0); iconPath: "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" }
                    IconButton { width: 30; height: 30; label: qsTr("Icons (Ctrl+2)"); checked: window.browser !== null && window.page === 0 && window.browser.view === 1; onClicked: window.browser.setView(1); iconPath: "M3.5 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM10 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM3.5 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM10 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1z" }
                    IconButton { width: 30; height: 30; label: qsTr("Columns (Ctrl+3)"); checked: window.browser !== null && window.page === 0 && window.browser.view === 2; onClicked: window.browser.setView(2); iconPath: "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM8 3v10" }
                }
            }
            Button { id: newButton; Layout.leftMargin: 12; text: qsTr("New"); menu: true; onClicked: newMenu.popup(newButton, 0, newButton.height + 4); iconPath: "M2 4.2c0-.6.5-1.2 1.2-1.2h3.3l1.5 1.5h4.8c.7 0 1.2.5 1.2 1.2v6.1c0 .7-.5 1.2-1.2 1.2H3.2c-.7 0-1.2-.5-1.2-1.2zM8 7v4M6 9h4" }
            Rectangle {
                Layout.leftMargin: 12
                color: Theme.field; radius: Theme.radius
                implicitWidth: tools.implicitWidth; implicitHeight: Theme.controlSize
                Row {
                    id: tools
                    IconButton { id: appearanceButton; label: qsTr("Appearance"); onClicked: appearanceMenu.popup(appearanceButton, 0, appearanceButton.height + 4); iconPath: "M8 2.5a5.5 5.5 0 0 0 0 11c.9 0 1.2-.6 1-1.3-.3-.9.2-1.7 1.2-1.7h1.3a2 2 0 0 0 2-2A5.8 5.8 0 0 0 8 2.5zM5.5 7h.01M8 5h.01" }
                    IconButton { label: qsTr("Settings (Ctrl+,)"); onClicked: settingsDialog.open(); iconPath: "M2.5 5h7M12.5 5h1M2.5 11h1M6.5 11h7M11 3.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zM5 9.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z" }
                }
            }
            IconButton {
                Layout.leftMargin: 6
                visible: AppSettings.windowButtons
                label: qsTr("Minimize")
                iconPath: "M4 8h8"
                onClicked: window.showMinimized()
            }
            IconButton {
                visible: AppSettings.windowButtons
                label: window.visibility === Window.Maximized ? qsTr("Restore") : qsTr("Maximize")
                iconPath: window.visibility === Window.Maximized ? "M5.5 3.5h7v7M3.5 5.5h7v7h-7z" : "M3.5 3.5h9v9h-9z"
                onClicked: window.visibility === Window.Maximized ? window.showNormal() : window.showMaximized()
            }
            IconButton { Layout.leftMargin: AppSettings.windowButtons ? 0 : 6; label: qsTr("Close"); iconPath: "m4 4 8 8M12 4l-8 8"; onClicked: window.close() }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Sidebar: full, an icon rail, or hidden.
            ColumnLayout {
                id: sidebar
                visible: window.sidebarMode !== 2
                readonly property bool compact: window.sidebarMode === 1
                readonly property int barWidth: compact ? Theme.railWidth : Theme.sidebarWidth
                Layout.preferredWidth: barWidth
                Layout.minimumWidth: barWidth
                Layout.maximumWidth: barWidth
                Layout.fillHeight: true
                spacing: 0

                ListView {
                    id: placesList
                    objectName: "places"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    model: window.places
                    spacing: 2
                    delegate: Column {
                        id: placeRow
                        // Roles are read through `model`: `icon` would clash with the button's own.
                        required property var model
                        readonly property color sectionColor: Theme.sectionColor(model.section, window.places.sectionColors)
                        // Only the Folders can be put in another order.
                        readonly property bool movable: model.section === 1 && !sidebar.compact
                        width: ListView.view.width
                        spacing: 2

                        // A section's first place carries its heading (the pages at the top have
                        // none); in the rail, a short rule.
                        SectionHeading {
                            visible: !sidebar.compact && placeRow.model.firstInSection && placeRow.model.section > 0
                            width: parent.width
                            title: visible ? [qsTr("Folders"), qsTr("Pinned"), qsTr("Storage")][placeRow.model.section - 1] : ""
                            color: placeRow.sectionColor
                        }
                        Rectangle {
                            visible: sidebar.compact && placeRow.model.firstInSection && placeRow.model.section > 0
                            x: 14; width: parent.width - 28; height: 1
                            color: placeRow.sectionColor
                            opacity: 0.45
                        }
                        PlaceItem {
                            id: placeItem
                            width: parent.width
                            compact: sidebar.compact
                            text: placeRow.model.name
                            iconPath: placeRow.model.icon
                            iconFilled: placeRow.model.page === 3
                            sectionColor: window.places.coloredIcons || selected ? placeRow.sectionColor : Theme.textDim
                            used: placeRow.model.section === 3 && placeRow.model.page === 0 ? placeRow.model.used : -1
                            badge: placeRow.model.page === 2 ? (window.recent.count > 0 ? String(window.recent.count) : "")
                                 : placeRow.model.page === 4 ? (window.trash.count > 0 ? String(window.trash.count) : "")
                                 : placeRow.model.section === 3 ? placeRow.model.freeText : ""
                            selected: window.browser !== null && (placeRow.model.page !== 0 ? window.page === placeRow.model.page
                                                                                             : window.page === 0 && window.browser.location === placeRow.model.path)
                            dropTarget: drop.containsDrag
                            onClicked: placeRow.model.page !== 0 ? window.browser.goPage(placeRow.model.page) : window.browser.goPath(placeRow.model.path)

                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                onTapped: placeMenu.show(placeRow.model, placeRow.sectionColor)
                            }
                            DragHandler {
                                id: drag
                                enabled: placeRow.movable
                                target: null
                                onActiveChanged: {
                                    if (active) {
                                        ghost.key = placeRow.model.key
                                        ghost.text = placeRow.model.name
                                        ghost.visible = true
                                    } else {
                                        ghost.Drag.drop()
                                        ghost.visible = false
                                    }
                                }
                                onCentroidChanged: {
                                    if (active) {
                                        const at = placeItem.mapToItem(window.contentItem, centroid.position)
                                        ghost.x = at.x - 20
                                        ghost.y = at.y - ghost.height / 2
                                    }
                                }
                            }
                            DropArea {
                                id: drop
                                anchors.fill: parent
                                enabled: placeRow.movable
                                keys: ["asc-files-place"]
                                onDropped: (event) => window.places.moveFolder(event.source.key, placeRow.model.key)
                            }
                            // Files dropped on a folder or drive go there.
                            DropArea {
                                anchors.fill: parent
                                enabled: placeRow.model.path.length > 0
                                keys: ["text/uri-list"]
                                onDropped: (event) => { if (window.transfers.drop(event.urls, placeRow.model.path)) event.accept() }
                            }
                        }
                    }
                }
                Item { Layout.preferredHeight: 18 }
            }

            // Content panel
            Rectangle {
                objectName: "panel"
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.rightMargin: 10
                Layout.bottomMargin: 10
                Layout.leftMargin: sidebar.visible ? 0 : 10
                radius: Theme.panelRadius
                color: Theme.panel
                border.width: 1
                border.color: Theme.panelEdge

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 1
                    spacing: 0

                    RowLayout {
                        Layout.preferredHeight: 46
                        Layout.leftMargin: 16
                        Layout.rightMargin: 16
                        spacing: 4
                        Repeater {
                            model: window.tabs
                            TabChip {
                                required property int index
                                required property string title
                                Layout.alignment: Qt.AlignBottom
                                text: title
                                current: index === window.tabs.currentIndex
                                onClicked: window.tabs.currentIndex = index
                                onCloseRequested: window.tabs.closeTab(index)
                                onMenuRequested: tabMenu.show(index)
                            }
                        }
                        IconButton {
                            Layout.alignment: Qt.AlignBottom
                            width: 30; height: 30
                            label: qsTr("New tab (Ctrl+T)")
                            iconPath: "M8 3v10M3 8h10"
                            onClicked: window.tabs.openTab()
                        }
                        Item { Layout.fillWidth: true }
                    }
                    RowLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 0
                    StackLayout {
                        id: pages
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        currentIndex: window.page

                        FolderView {
                            id: folderView
                            browser: window.browser
                            transfers: window.transfers
                            ops: window.ops
                            onNotice: (message) => notice.show(message)
                            onContextRequested: (row) => row >= 0 ? itemMenu.show(row) : spaceMenu.popup()
                            onFileActivated: (path) => window.ops.open(path)
                            onPathMenuRequested: (path, isFolder) => pathMenu.show(path, isFolder)
                        }
                        OverviewPage {
                            browser: window.browser
                            places: window.places
                            recent: window.recent
                            favorites: window.favorites
                            trash: window.trash
                            downloads: window.downloads
                            onFileActivated: (path) => window.ops.open(path)
                            onMenuRequested: (path, isFolder) => pathMenu.show(path, isFolder)
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.recent
                            onFileActivated: (path) => window.ops.open(path)
                            onContextRequested: (row) => pathMenu.show(window.recent.pathAt(row), window.recent.isFolderAt(row))
                            whenTitle: qsTr("Used")
                            emptyText: qsTr("Files you open show up here")
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.favorites
                            onFileActivated: (path) => window.ops.open(path)
                            onContextRequested: (row) => pathMenu.show(window.favorites.pathAt(row), window.favorites.isFolderAt(row))
                            emptyText: qsTr("Star files and folders to keep them at hand")
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.trash
                            whenTitle: qsTr("Deleted")
                            folderTitle: qsTr("Deleted from")
                            emptyText: qsTr("The Trash is empty")
                            opensFolders: false
                            trashActions: true
                            onContextRequested: (row) => trashMenu.show(row)
                            onRestoreRequested: (row) => window.ops.restore(window.trash.pathAt(row))
                            onDeleteRequested: (row) => confirm.ask(qsTr("Delete “%1” for good?").arg(window.trash.nameAt(row)),
                                                                    qsTr("This cannot be undone."), qsTr("Delete"),
                                                                    () => window.ops.deleteFromTrash(window.trash.extraAt(row)))
                            onEmptyRequested: confirm.ask(qsTr("Empty the Trash?"),
                                                          qsTr("Everything in the Trash, on every drive, is deleted for good. This cannot be undone."),
                                                          qsTr("Empty Trash"), () => window.ops.emptyTrash())
                        }
                        SearchPage {
                            id: searchPage
                            browser: window.browser
                            search: window.search
                            places: window.places
                            onFileActivated: (path) => window.ops.open(path)
                            onContextRequested: (row) => pathMenu.show(window.search.pathAt(row), window.search.isFolderAt(row))
                        }
                    }
                    Rectangle {
                        objectName: "previewPanel"
                        visible: window.previewOpen && window.previewAsPanel
                        Layout.preferredWidth: 380
                        Layout.fillHeight: true
                        color: "transparent"
                        Rectangle { width: 1; height: parent.height; color: Theme.rule }
                        PreviewContent {
                            anchors.fill: parent
                            anchors.leftMargin: 1
                            preview: window.preview
                            onCloseRequested: window.previewOpen = false
                        }
                    }
                    }
                    JobsIndicator {
                        Layout.leftMargin: 18
                        Layout.topMargin: 6
                        Layout.bottomMargin: -26
                        jobs: window.jobModel
                    }
                    Text {
                        Layout.alignment: Qt.AlignRight
                        Layout.rightMargin: 18
                        Layout.bottomMargin: 14
                        Layout.topMargin: 6
                        text: window.view ? window.view.status : ""
                        color: Theme.textSoft
                        font.family: Theme.family
                        font.pointSize: Theme.small
                    }
                }
            }
        }
    }

    Connections {
        target: window.jobModel
        function onFinished(job, summary) {
            if (summary.failed > 0)
                notice.show(window.jobModel.errorOf(job))
        }
    }

    readonly property bool inFolder: browser !== null && page === 0
    readonly property int selectionCount: inFolder && folderModel ? folderModel.selectionCount : 0

    function deleteSelection() {
        if (selectionCount === 0) return
        confirm.ask(qsTr("Delete %n items for good?", "", selectionCount),
                    qsTr("They will not go to the Trash, and this cannot be undone."),
                    qsTr("Delete"), () => window.ops.deleteForever(window.folderModel))
    }
    function openRow(row) {
        if (window.folderModel.isFolder(row)) window.browser.goRow(row)
    }

    Connections {
        target: window.ops
        enabled: window.foremost
        function onNotice(message) { notice.show(message) }
        function onTrashChanged() { window.trash.reload() }
        function onUndoBlocked(names) {
            confirm.ask(qsTr("Put back over newer items?"),
                        qsTr("Undo would put items back where these are now: %1. They will go to the Trash, so nothing is lost.").arg(names.join(", ")),
                        qsTr("Move to Trash and Undo"), () => window.ops.undo(true))
        }
    }
    ConflictDialog { ops: window.ops; listening: window.foremost }
    ConfirmDialog { id: confirm }
    SettingsDialog {
        id: settingsDialog
        places: window.places
        theme: SystemTheme
        currentFolder: window.inFolder ? window.browser.location : ""
    }

    // A right click on an item named by its path: in Recent, Favorites, search results, the
    // Overview, a crumb, a column other than the current folder's.
    ContextMenu {
        id: pathMenu
        property string path
        property bool isFolder
        /// A crumb: an ancestor of the folder shown, not offered for the Trash.
        property bool crumb
        readonly property string parentFolder: path.substring(0, path.lastIndexOf("/")) || "/"
        function show(at, folder, isCrumb) { path = at; isFolder = folder; crumb = isCrumb === true; popup() }
        MenuItem {
            text: qsTr("Open")
            onTriggered: pathMenu.isFolder ? window.browser.goPath(pathMenu.path) : window.ops.open(pathMenu.path)
        }
        MenuItem {
            text: qsTr("Open in New Tab")
            visible: pathMenu.isFolder
            height: visible ? implicitHeight : 0
            onTriggered: window.tabs.openTab(pathMenu.path)
        }
        MenuItem {
            text: qsTr("Open in New Window")
            visible: pathMenu.isFolder && window.windows !== null
            height: visible ? implicitHeight : 0
            onTriggered: window.windows.open([pathMenu.path])
        }
        MenuItem {
            text: qsTr("Show in Folder")
            visible: !pathMenu.crumb && pathMenu.path !== "/"
            height: visible ? implicitHeight : 0
            onTriggered: window.browser.goPath(pathMenu.parentFolder)
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Cut"); visible: !pathMenu.crumb; height: visible ? implicitHeight : 0; onTriggered: window.ops.copyPaths([pathMenu.path], true) }
        MenuItem { text: qsTr("Copy"); onTriggered: window.ops.copyPaths([pathMenu.path], false) }
        MenuItem { text: qsTr("Copy Location"); onTriggered: window.ops.copyPathText([pathMenu.path]) }
        MenuSeparator {}
        MenuItem {
            text: window.favorites.contains(pathMenu.path) ? qsTr("Remove from Favorites") : qsTr("Add to Favorites")
            onTriggered: window.favorites.contains(pathMenu.path) ? window.favorites.remove(pathMenu.path) : window.favorites.add(pathMenu.path)
        }
        MenuItem {
            text: window.places.isPinned(pathMenu.path) ? qsTr("Unpin from Sidebar") : qsTr("Pin to Sidebar")
            visible: pathMenu.isFolder
            height: visible ? implicitHeight : 0
            onTriggered: window.places.isPinned(pathMenu.path) ? window.places.unpin(pathMenu.path) : window.places.pin(pathMenu.path)
        }
        MenuSeparator { visible: !pathMenu.crumb; height: visible ? implicitHeight : 0 }
        MenuItem {
            text: qsTr("Move to Trash")
            visible: !pathMenu.crumb
            height: visible ? implicitHeight : 0
            onTriggered: window.ops.trashPaths([pathMenu.path])
        }
    }

    // A right click on an item in the Trash.
    ContextMenu {
        id: trashMenu
        property int row: -1
        function show(at) { row = at; popup() }
        MenuItem { text: qsTr("Restore"); onTriggered: window.ops.restore(window.trash.pathAt(trashMenu.row)) }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Delete for Good…")
            onTriggered: {
                const row = trashMenu.row
                confirm.ask(qsTr("Delete “%1” for good?").arg(window.trash.nameAt(row)), qsTr("This cannot be undone."),
                            qsTr("Delete"), () => window.ops.deleteFromTrash(window.trash.extraAt(row)))
            }
        }
    }

    // A right click on a tab.
    ContextMenu {
        id: tabMenu
        property int index: -1
        function show(at) { index = at; popup() }
        MenuItem { text: qsTr("New Tab"); onTriggered: window.tabs.openTab() }
        MenuItem { text: qsTr("Duplicate Tab"); onTriggered: window.tabs.duplicateTab(tabMenu.index) }
        MenuItem {
            text: qsTr("Open in New Window")
            visible: window.windows !== null && window.tabs.folderAt(tabMenu.index).length > 0
            height: visible ? implicitHeight : 0
            onTriggered: window.windows.open([window.tabs.folderAt(tabMenu.index)])
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Close Tab"); enabled: window.tabs.count > 1; onTriggered: window.tabs.closeTab(tabMenu.index) }
        MenuItem { text: qsTr("Close Other Tabs"); enabled: window.tabs.count > 1; onTriggered: window.tabs.closeOtherTabs(tabMenu.index) }
        MenuItem {
            text: qsTr("Close Tabs to the Right")
            enabled: tabMenu.index < window.tabs.count - 1
            onTriggered: window.tabs.closeTabsAfter(tabMenu.index)
        }
    }

    // A right click on items.
    ContextMenu {
        id: itemMenu
        property int row: -1
        readonly property bool single: window.selectionCount === 1
        readonly property bool folder: row >= 0 && window.folderModel !== null && window.folderModel.isFolder(row)
        readonly property string path: row >= 0 && window.folderModel !== null ? window.folderModel.pathAt(row) : ""
        function show(at) { row = at; popup() }
        MenuItem { text: qsTr("Open"); visible: itemMenu.single && itemMenu.folder; height: visible ? implicitHeight : 0; onTriggered: window.openRow(itemMenu.row) }
        MenuItem {
            text: qsTr("Open in New Tab")
            visible: itemMenu.single && itemMenu.folder
            height: visible ? implicitHeight : 0
            onTriggered: window.tabs.openTab(itemMenu.path)
        }
        MenuItem {
            text: qsTr("Open in New Window")
            visible: itemMenu.single && itemMenu.folder && window.windows !== null
            height: visible ? implicitHeight : 0
            onTriggered: window.windows.open([itemMenu.path])
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Cut"); onTriggered: window.ops.copy(window.folderModel, true) }
        MenuItem { text: qsTr("Copy"); onTriggered: window.ops.copy(window.folderModel, false) }
        MenuItem {
            text: qsTr("Paste Into Folder")
            visible: itemMenu.single && itemMenu.folder
            height: visible ? implicitHeight : 0
            enabled: window.ops.canPaste
            onTriggered: window.ops.paste(window.folderModel, itemMenu.row)
        }
        MenuItem { text: qsTr("Rename…"); visible: itemMenu.single; height: visible ? implicitHeight : 0; onTriggered: folderView.startRename() }
        MenuSeparator {}
        MenuItem {
            text: window.favorites.contains(itemMenu.path) ? qsTr("Remove from Favorites") : qsTr("Add to Favorites")
            visible: itemMenu.single
            height: visible ? implicitHeight : 0
            onTriggered: window.favorites.contains(itemMenu.path) ? window.favorites.remove(itemMenu.path) : window.favorites.add(itemMenu.path)
        }
        MenuItem {
            text: qsTr("Pin to Sidebar")
            visible: itemMenu.single && itemMenu.folder && !window.places.isPinned(itemMenu.path)
            height: visible ? implicitHeight : 0
            onTriggered: window.places.pin(itemMenu.path)
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Move to Trash"); onTriggered: window.ops.trash(window.folderModel) }
        MenuItem { text: qsTr("Delete for Good…"); onTriggered: window.deleteSelection() }
    }

    // A right click on empty space in a folder.
    ContextMenu {
        id: spaceMenu
        MenuItem { text: qsTr("New Folder"); onTriggered: window.ops.create(window.folderModel, true) }
        MenuItem { text: qsTr("New File"); onTriggered: window.ops.create(window.folderModel, false) }
        MenuSeparator {}
        MenuItem { text: qsTr("Paste"); enabled: window.ops.canPaste; onTriggered: window.ops.paste(window.folderModel) }
        MenuItem { text: qsTr("Select All"); onTriggered: window.folderModel.selectAll() }
        MenuSeparator {}
        MenuItem { text: window.ops.undoLabel.length > 0 ? qsTr("Undo %1").arg(window.ops.undoLabel) : qsTr("Undo"); enabled: window.ops.undoLabel.length > 0; onTriggered: window.ops.undo() }
        MenuItem { text: window.ops.redoLabel.length > 0 ? qsTr("Redo %1").arg(window.ops.redoLabel) : qsTr("Redo"); enabled: window.ops.redoLabel.length > 0; onTriggered: window.ops.redo() }
        MenuSeparator {}
        MenuItem { text: qsTr("Show Hidden Files"); checkable: true; checked: window.folderModel !== null && window.folderModel.showHidden; onTriggered: window.folderModel.showHidden = !window.folderModel.showHidden }
    }

    // The header's New menu.
    ContextMenu {
        id: newMenu
        MenuItem { text: qsTr("New Folder"); enabled: window.inFolder; onTriggered: window.ops.create(window.folderModel, true) }
        MenuItem { text: qsTr("New File"); enabled: window.inFolder; onTriggered: window.ops.create(window.folderModel, false) }
        MenuSeparator {}
        MenuItem { text: qsTr("New Tab"); onTriggered: window.tabs.openTab() }
        MenuItem { text: qsTr("New Window"); enabled: window.windows !== null; onTriggered: window.windows.open([window.browser.location]) }
    }

    Popup {
        id: previewPopup
        objectName: "previewPopup"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(window.width - 120, 1100)
        height: window.height - 120
        visible: window.previewOpen && !window.previewAsPanel
        modal: false
        focus: visible
        padding: 0
        closePolicy: Popup.CloseOnEscape
        onClosed: window.previewOpen = false
        background: Rectangle { radius: 16; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }
        contentItem: PreviewContent {
            preview: window.preview
            onCloseRequested: window.previewOpen = false
            focus: true
            // Keys go to the drawer while it is open: Space closes it, the arrows move along.
            Keys.onSpacePressed: window.previewOpen = false
            Keys.onLeftPressed: window.view.step(-1)
            Keys.onUpPressed: window.view.step(-1)
            Keys.onRightPressed: window.view.step(1)
            Keys.onDownPressed: window.view.step(1)
        }
    }

    // The header's Appearance menu.
    ContextMenu {
        id: appearanceMenu
        MenuItem { text: qsTr("Preview as Popup"); checkable: true; checked: !window.previewAsPanel; onTriggered: AppSettings.previewStyle = "popup" }
        MenuItem { text: qsTr("Preview as Panel"); checkable: true; checked: window.previewAsPanel; onTriggered: AppSettings.previewStyle = "panel" }
        MenuSeparator {}
        MenuItem { text: qsTr("Show Hidden Files"); checkable: true; enabled: window.inFolder; checked: window.folderModel !== null && window.folderModel.showHidden; onTriggered: window.folderModel.showHidden = !window.folderModel.showHidden }
    }

    Notice {
        id: notice
        objectName: "notice"
        z: 50
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 28
    }

    // What a sidebar folder being dragged to a new place shows under the pointer.
    Rectangle {
        id: ghost
        property string key
        property alias text: ghostLabel.text
        visible: false
        z: 100
        width: ghostLabel.implicitWidth + 28
        height: Theme.placeHeight
        radius: Theme.radius
        color: Theme.raised
        border.width: 1
        border.color: Theme.selectionEdge
        opacity: 0.92
        Drag.active: visible
        Drag.keys: ["asc-files-place"]
        Drag.hotSpot.x: 20
        Drag.hotSpot.y: height / 2
        Text {
            id: ghostLabel
            anchors.centerIn: parent
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.body
        }
    }

    // A place's menu: hide it, unpin it, show hidden places again, colour its section.
    ContextMenu {
        id: placeMenu
        property var place: ({})
        function show(model, color) {
            place = { key: model.key, name: model.name, section: model.section, path: model.path, hideable: model.hideable }
            popup()
        }
        MenuItem {
            text: qsTr("Open in New Tab")
            onTriggered: {
                window.tabs.openTab()
                const browser = window.tabs.current
                const pages = { overview: 1, recent: 2, favorites: 3, trash: 4 }
                if (pages[placeMenu.place.key] !== undefined) browser.goPage(pages[placeMenu.place.key])
                else browser.goPath(placeMenu.place.path)
            }
        }
        MenuItem {
            text: qsTr("Hide from Sidebar")
            visible: placeMenu.place.hideable === true
            height: visible ? implicitHeight : 0
            onTriggered: window.places.setHidden(placeMenu.place.key, true)
        }
        MenuItem {
            text: qsTr("Unpin")
            visible: placeMenu.place.section === 2
            height: visible ? implicitHeight : 0
            onTriggered: window.places.unpin(placeMenu.place.path)
        }
        ContextMenu {
            title: qsTr("Show Hidden Places")
            enabled: window.places.hiddenPlaces.length > 0
            Repeater {
                model: window.places.hiddenPlaces
                MenuItem {
                    required property var modelData
                    text: modelData.name
                    onTriggered: window.places.setHidden(modelData.key, false)
                }
            }
        }
        ContextMenu {
            title: qsTr("Section Colour")
            enabled: placeMenu.place.section > 0
            MenuItem {
                text: qsTr("Theme colour")
                onTriggered: window.places.setSectionColor(placeMenu.place.section, "")
            }
            Repeater {
                model: Theme.swatchNames
                MenuItem {
                    required property string modelData
                    text: modelData.charAt(0).toUpperCase() + modelData.slice(1)
                    checkable: true
                    checked: window.places.sectionColors[String(placeMenu.place.section)] === modelData
                    onTriggered: window.places.setSectionColor(placeMenu.place.section, modelData)
                }
            }
        }
        MenuItem {
            text: qsTr("Coloured Icons")
            checkable: true
            checked: window.places.coloredIcons
            onTriggered: window.places.coloredIcons = !window.places.coloredIcons
        }
        MenuItem {
            text: qsTr("Reset Folder Order")
            onTriggered: window.places.resetFolderOrder()
        }
    }

    Shortcut { sequences: ["Alt+Left", "Backspace"]; enabled: !breadcrumb.editing; onActivated: window.browser.back() }
    Shortcut { sequence: "Alt+Right"; onActivated: window.browser.forward() }
    Shortcut { sequence: "Alt+Up"; onActivated: window.browser.up() }
    Shortcut { sequence: "Ctrl+L"; onActivated: breadcrumb.edit() }
    Shortcut { sequence: "Ctrl+T"; onActivated: window.tabs.openTab() }
    Shortcut { sequence: "Ctrl+W"; onActivated: window.tabs.count > 1 ? window.tabs.closeTab(window.tabs.currentIndex) : window.close() }
    Shortcut { sequence: "Ctrl+N"; enabled: window.windows !== null; onActivated: window.windows.open([window.browser.location]) }
    Shortcut { sequence: "Ctrl+Shift+W"; onActivated: window.close() }
    Shortcut { sequence: "Ctrl+Tab"; onActivated: window.tabs.next() }
    Shortcut { sequence: "Ctrl+Shift+Tab"; onActivated: window.tabs.previous() }
    Shortcut { sequence: "Ctrl+B"; onActivated: window.places.stepMode() }
    Shortcut { sequence: "Ctrl+H"; onActivated: window.folderModel.showHidden = !window.folderModel.showHidden }
    Shortcut { sequences: [StandardKey.Copy]; enabled: window.selectionCount > 0; onActivated: window.ops.copy(window.folderModel, false) }
    Shortcut { sequences: [StandardKey.Cut]; enabled: window.selectionCount > 0; onActivated: window.ops.copy(window.folderModel, true) }
    Shortcut { sequences: [StandardKey.Paste]; enabled: window.inFolder; onActivated: window.ops.paste(window.folderModel) }
    Shortcut { sequence: "Delete"; enabled: window.selectionCount > 0; onActivated: window.ops.trash(window.folderModel) }
    Shortcut { sequence: "Shift+Delete"; enabled: window.selectionCount > 0; onActivated: window.deleteSelection() }
    Shortcut { sequences: [StandardKey.Undo]; onActivated: window.ops.undo() }
    Shortcut { sequences: [StandardKey.Redo, "Ctrl+Shift+Z"]; onActivated: window.ops.redo() }
    Shortcut { sequence: "Ctrl+Shift+N"; enabled: window.inFolder; onActivated: window.ops.create(window.folderModel, true) }
    Shortcut { sequence: "Space"; enabled: !previewPopup.visible; onActivated: window.previewOpen = !window.previewOpen }
    Shortcut { sequence: "Ctrl+,"; onActivated: settingsDialog.open() }
    Shortcut { sequence: "Ctrl+K"; onActivated: { searchField.forceActiveFocus(); searchField.selectAll() } }
    Shortcut { sequence: "Ctrl+1"; onActivated: window.browser.setView(0) }
    Shortcut { sequence: "Ctrl+2"; onActivated: window.browser.setView(1) }
    Shortcut { sequence: "Ctrl+3"; onActivated: window.browser.setView(2) }
}
