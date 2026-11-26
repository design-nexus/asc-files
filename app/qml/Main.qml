// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Dialogs

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
    /// The desktop's programs, for Open With.
    required property var apps
    /// What Properties shows, the window's own.
    required property var properties
    /// The user's custom actions and their runs (the app's).
    required property var actions
    /// Whether a newer Files is out (none in tests).
    property var updates: null
    Connections {
        target: window.updates
        enabled: window.foremost
        function onFound(version) { notice.show(qsTr("Files %1 is available (Settings › About)").arg(version)) }
    }
    /// The folders opened, for 10xer's z / Z (none in tests).
    property var visits: null
    Connections {
        target: window.browser
        enabled: window.visits !== null && window.foremost
        function onLocationChanged() {
            if (window.browser.page === 0 && window.browser.location.startsWith("/"))
                window.visits.record(window.browser.location)
        }
    }
    /// Searches below the folder shown (10xer's s), in the Search page.
    function searchHere(query) {
        const here = window.browser.location
        searchPage.origin = here
        window.browser.goPage(5)
        window.search.search(query, [here])
        window.search.searchNow()
    }
    /// Cloud accounts through Nyx Cloud (none in tests).
    property var cloud: null
    /// Servers through gvfs (none in tests).
    property var network: null
    /// The drives UDisks2 knows (none in tests).
    property var disks: null
    /// Settings › Integrations (none in tests).
    property var integrations: null
    /// A file chooser's request (another program asked Files to choose), or null.
    property var chooser: null
    /// Actions offered for what was right-clicked last, by placement.
    readonly property var topActions: actions.offered.filter(action => action.menu === "top")
    readonly property var submenuActions: actions.offered.filter(action => action.menu !== "top")

    // Runs an action, asking first when it says to.
    function runAction(action, paths, parent, background) {
        const go = () => window.actions.run(action.id, paths, parent, background)
        if (action.confirm)
            confirm.ask(qsTr("Run “%1”?").arg(action.name),
                        qsTr("It runs on %n items, with your permissions.", "", paths.length), qsTr("Run"), go)
        else
            go()
    }
    /// The app's windows (absent in a test of one window).
    property var windows: null
    /// Whether operations answer here: the window used last does (their notices, clashes and
    /// questions show in one window only).
    readonly property bool foremost: windows === null || windows.current === window

    /// The tab's browser, which holds its view; `browser` is the pane in use (Dual Pane's
    /// other pane when that one is), which the crumbs, menus and operations follow.
    readonly property var tabBrowser: tabs.current
    readonly property bool dualPane: tabBrowser !== null && tabBrowser.page === 0 && tabBrowser.view === 5 && tabBrowser.second !== null
    property bool rightActive: false
    onTabBrowserChanged: rightActive = false
    readonly property var browser: dualPane && rightActive ? tabBrowser.second : tabBrowser
    readonly property bool gallery: tabBrowser !== null && tabBrowser.page === 0 && tabBrowser.view === 4
    readonly property bool iconsShown: tabBrowser !== null && tabBrowser.page === 0 && tabBrowser.view === 1
    readonly property var folderModel: browser ? browser.folderModel : null

    width: 1600
    height: 968
    visible: true
    color: Theme.window
    title: chooser ? chooser.title : browser ? browser.title : ""

    // The view the current tab shows, by its page (Browser.Folder, Overview, Recent, Favorites,
    // Trash, in that order).
    readonly property int page: browser ? browser.page : 0
    readonly property var view: page === 0 ? (dualPane && rightActive && rightPaneLoader.item ? rightPaneLoader.item : folderView) : pages.children[page]
    // The preview drawer: shown as a Popup over the window (as legacy's default) or a Panel beside
    // the folder, and following the item the keyboard is on.
    property bool previewOpen: false
    readonly property bool previewAsPanel: AppSettings.previewStyle === "panel"
    // Gallery shows the preview large in the folder.
    readonly property string previewPath: (previewOpen || gallery) && view ? view.currentPath : ""
    onPreviewPathChanged: preview.show(previewPath)
    // Equations in documents are drawn in the text's colour.
    Binding {
        target: window.preview
        property: "ink"
        value: "#" + [Theme.text.r, Theme.text.g, Theme.text.b].map(v => Math.round(v * 255).toString(16).padStart(2, "0")).join("")
    }

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
                network: window.network
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
                // 10xer searches with s; Ctrl+K still shows the field.
                visible: !AppSettings.tenxer || window.searchWanted || text.length > 0
                onActiveFocusChanged: if (!activeFocus) window.searchWanted = false
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
                    IconButton { width: 30; height: 30; label: qsTr("List (Ctrl+1)"); checked: window.browser !== null && window.page === 0 && window.tabBrowser.view === 0; onClicked: window.tabBrowser.view = 0; iconPath: "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" }
                    IconButton { width: 30; height: 30; label: qsTr("Icons (Ctrl+2)"); checked: window.browser !== null && window.page === 0 && window.tabBrowser.view === 1; onClicked: window.tabBrowser.view = 1; iconPath: "M3.5 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM10 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM3.5 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM10 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1z" }
                    IconButton { width: 30; height: 30; label: qsTr("Columns (Ctrl+3)"); checked: window.browser !== null && window.page === 0 && window.tabBrowser.view === 2; onClicked: window.tabBrowser.view = 2; iconPath: "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM8 3v10" }
                    IconButton {
                        id: moreViews
                        width: 30; height: 30
                        label: qsTr("More views")
                        checked: window.tabBrowser !== null && window.page === 0 && window.tabBrowser.view >= 3
                        iconPath: window.tabBrowser !== null && window.tabBrowser.view === 4 ? "M2.5 3h11v7h-11zM2.5 12.5h2M7 12.5h2M11.5 12.5h2"
                                : window.tabBrowser !== null && window.tabBrowser.view === 5 ? "M2.5 3h11v10h-11zM8 3v10"
                                : window.tabBrowser !== null && window.tabBrowser.view === 3 ? "M2.5 4h4M2.5 8h4M2.5 12h4M9.5 4h4M9.5 8h4M9.5 12h4"
                                : "m4 6 4 4 4-4"
                        onClicked: viewsMenu.popup(moreViews, 0, moreViews.height + 4)
                    }
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
                            onClicked: placeRow.model.page !== 0 ? window.browser.goPage(placeRow.model.page)
                                     : placeRow.model.path.length > 0 ? window.browser.goPath(placeRow.model.path)
                                     : placeRow.model.cloud.length > 0 ? window.cloud.connectAccount(placeRow.model.cloud)
                                     : window.openVolume(placeRow.model.volume, placeRow.model.locked, placeRow.model.name)

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
                                onDropped: (event) => {
                                    const result = window.transfers.drop(event.urls, placeRow.model.path)
                                    if (result !== Transfers.Refused) { event.accept(); window.dropped(result, placeRow.model.path) }
                                }
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

                        // The folder: one pane, or two side by side (Dual Pane).
                        RowLayout {
                            spacing: 0
                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.preferredWidth: 1
                                spacing: 0
                                PaneBar { visible: window.dualPane; browser: window.tabBrowser; active: !window.rightActive }
                                FolderView {
                                    id: folderView
                                    // A focus scope: the folder takes the keyboard as the window opens.
                                    focus: true
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    browser: window.tabBrowser
                                    forcedMode: window.dualPane ? 0 : -1
                                    preview: window.preview
                                    transfers: window.transfers
                                    ops: window.ops
                                    onActiveFocusChanged: if (activeFocus) window.rightActive = false
                                    onNotice: (message) => notice.show(message)
                                    onContextRequested: (row) => { window.rightActive = false; row >= 0 ? itemMenu.show(row) : spaceMenu.show() }
                                    onFileActivated: (path) => window.openFile(path)
                                    onPathMenuRequested: (path, isFolder) => pathMenu.show(path, isFolder)
                                    onDropped: (result, target) => window.dropped(result, target)
                                }
                            }
                            Rectangle { visible: window.dualPane; Layout.fillHeight: true; width: 1; color: Theme.rule }
                            ColumnLayout {
                                visible: window.dualPane
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.preferredWidth: 1
                                spacing: 0
                                PaneBar { browser: window.tabBrowser ? window.tabBrowser.second : null; active: window.rightActive }
                                Loader {
                                    id: rightPaneLoader
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    active: window.dualPane
                                    sourceComponent: FolderView {
                                        objectName: "rightPane"
                                        browser: window.tabBrowser.second
                                        forcedMode: 0
                                        transfers: window.transfers
                                        ops: window.ops
                                        onActiveFocusChanged: if (activeFocus) window.rightActive = true
                                        onNotice: (message) => notice.show(message)
                                        onContextRequested: (row) => { window.rightActive = true; row >= 0 ? itemMenu.show(row) : spaceMenu.show() }
                                        onFileActivated: (path) => window.openFile(path)
                                        onPathMenuRequested: (path, isFolder) => pathMenu.show(path, isFolder)
                                        onDropped: (result, target) => window.dropped(result, target)
                                    }
                                }
                            }
                        }
                        OverviewPage {
                            browser: window.browser
                            places: window.places
                            recent: window.recent
                            favorites: window.favorites
                            trash: window.trash
                            downloads: window.downloads
                            onFileActivated: (path) => window.openFile(path)
                            onMenuRequested: (path, isFolder) => pathMenu.show(path, isFolder)
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.recent
                            onFileActivated: (path) => window.openFile(path)
                            onContextRequested: (row) => pathMenu.show(window.recent.pathAt(row), window.recent.isFolderAt(row))
                            whenTitle: qsTr("Used")
                            emptyText: qsTr("Files you open show up here")
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.favorites
                            onFileActivated: (path) => window.openFile(path)
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
                            onFileActivated: (path) => window.openFile(path)
                            onContextRequested: (row) => pathMenu.show(window.search.pathAt(row), window.search.isFolderAt(row))
                        }
                    }
                    Rectangle {
                        objectName: "previewPanel"
                        id: previewPanel
                        visible: window.previewOpen && window.previewAsPanel
                        // Its width is the user's: drag its left edge (Settings keeps it).
                        property int liveWidth: AppSettings.previewWidth
                        Layout.preferredWidth: Math.min(panelSizing.active ? liveWidth : AppSettings.previewWidth, window.width * 0.6)
                        Layout.fillHeight: true
                        color: "transparent"
                        Rectangle { width: 1; height: parent.height; color: panelEdgeHover.hovered || panelSizing.active ? Theme.selectionEdge : Theme.rule }
                        Item {
                            objectName: "previewPanelEdge"
                            width: 8
                            height: parent.height
                            x: -4
                            z: 2
                            property int startWidth
                            HoverHandler { id: panelEdgeHover; cursorShape: Qt.SplitHCursor }
                            DragHandler {
                                id: panelSizing
                                target: null
                                yAxis.enabled: false
                                onActiveChanged: {
                                    if (active) parent.startWidth = previewPanel.width
                                    else AppSettings.previewWidth = previewPanel.liveWidth
                                }
                                onTranslationChanged: if (active) previewPanel.liveWidth = Math.max(260, Math.min(900, parent.startWidth - translation.x))
                            }
                            TapHandler { onDoubleTapped: AppSettings.previewWidth = 380 }
                        }
                        PreviewContent {
                            anchors.fill: parent
                            anchors.leftMargin: 1
                            preview: window.preview
                            onCloseRequested: window.previewOpen = false
                        }
                    }
                    }
                    JobsIndicator {
                        id: jobsIndicator
                        actions: window.actions
                        onShowRequested: (path) => window.browser.goPath(path.substring(0, path.lastIndexOf("/")) || "/")
                        Layout.leftMargin: 18
                        Layout.topMargin: 6
                        Layout.bottomMargin: -26
                        jobs: window.jobModel
                    }
                    TenXerBar {
                        Layout.fillWidth: true
                        Layout.rightMargin: 18
                        Layout.leftMargin: 18
                        Layout.bottomMargin: 14
                        Layout.topMargin: 6
                        keys: tenxerKeys
                        status: window.view ? window.view.status : ""
                    }
                }
            }
        }
        ChooserBar {
            id: chooserBar
            visible: window.chooser !== null
            Layout.fillWidth: true
            job: window.chooser ? window.chooser : { kind: "", choices: [], filters: [], names: [], currentFilter: 0, acceptLabel: "", currentName: "" }
            onAccepted: window.chooserAccept(false)
            onCancelled: window.chooser.cancel()
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
    /// Selects `paths` once they show, with their Properties when asked: "Show in folder" from
    /// another program (FileManager1).
    function revealItems(paths, properties) {
        folderView.reveal(paths)
        if (properties && paths.length === 1) propertiesDialog.show(paths[0])
    }

    /// Answers the chooser with what is chosen: the selection (or the folder), or the name typed;
    /// what stands in the way is said, and replacing a file is asked first.
    function chooserAccept(replace) {
        const location = window.browser.location
        if (window.chooser.kind === "open") {
            const why = window.chooser.acceptOpen(window.folderModel.selectedPaths(), location, chooserBar.values, chooserBar.filter)
            if (why.length > 0) notice.show(why)
            return
        }
        const answer = window.chooser.acceptSave(location, chooserBar.name, chooserBar.values, chooserBar.filter, replace)
        if (answer.error) {
            notice.show(answer.error)
        } else if (answer.replace) {
            confirm.ask(qsTr("Replace %1?").arg(answer.replace.map(name => "“" + name + "”").join(", ")),
                        qsTr("Saving replaces what is there now."), qsTr("Replace"), () => window.chooserAccept(true))
        }
    }
    /// A file opened: in an Open chooser it is the answer; saving, its name is taken.
    function openFile(path) {
        if (window.chooser && window.chooser.kind === "open" && !window.chooser.directory) {
            const why = window.chooser.acceptOpen([path], window.browser.location, chooserBar.values, chooserBar.filter)
            if (why.length > 0) notice.show(why)
        } else if (window.chooser && window.chooser.kind === "saveFile") {
            chooserBar.setName(path.substring(path.lastIndexOf("/") + 1))
        } else if (!window.chooser) {
            window.ops.open(path)
        }
    }
    // Escape cancels a chooser, as a file chooser's does.
    Shortcut {
        sequence: "Escape"
        enabled: window.chooser !== null
        onActivated: window.chooser.cancel()
    }
    // A chooser's filter lists the folder by its globs.
    Binding {
        target: window.folderModel
        property: "globs"
        when: window.chooser !== null && window.folderModel !== null && window.chooser.filters.length > 0
        value: window.chooser && window.chooser.filters.length > 0 ? window.chooser.filters[chooserBar.filter].globs : []
    }

    /// A volume not mounted: mounted (unlocked first, asking its passphrase), then shown.
    // Pastes into this folder, remembering it for Send to when it is on a removable drive.
    function pasteHere() {
        if (window.disks && window.page === 0)
            window.disks.remember(window.browser.location)
        window.ops.paste(window.folderModel)
    }
        function openVolume(volume, locked, name) {
        if (!window.disks || volume.length === 0) return
        window.pendingVolume = volume
        if (locked) unlockDialog.ask(volume, name, false)
        else window.disks.mount(volume)
    }
    property string pendingVolume

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
    // Archives: Compress…, a locked archive's password, and where Extract To… goes.
    CompressDialog {
        id: compressDialog
        onChosen: (paths, name, format, password) => window.ops.compress(paths, name, format, password)
    }
    // A locked volume's passphrase, and a volume's new name.
    UnlockDialog {
        id: unlockDialog
        onGiven: (volume, passphrase) => window.disks.unlock(volume, passphrase)
    }
    NameDialog {
        id: connectServer
        title: qsTr("Connect to Server")
        actionText: qsTr("Connect")
        onGiven: (key, address) => window.network.connectTo(address)
    }
    ServerPasswordDialog {
        id: serverPassword
        onGiven: (asked, user, password, domain, anonymous, remember) => window.network.answerPassword(asked, user, password, domain, anonymous, remember)
        onCancelled: (asked) => window.network.cancel(asked)
    }
    QuestionDialog {
        id: serverQuestion
        onAnswered: (asked, choice) => window.network.answerQuestion(asked, choice)
        onCancelled: (asked) => window.network.cancel(asked)
    }
    Connections {
        target: window.cloud
        enabled: window.foremost
        function onOpened(path) { window.browser.goPath(path) }
        function onFailed(message) { notice.show(message) }
    }
    Connections {
        target: window.network
        enabled: window.foremost
        function onOpened(path) { window.browser.goPath(path) }
        function onFailed(message) { notice.show(message) }
        function onPasswordWanted(asked, message, user, domain, needsUser, needsDomain, anonymousAllowed, canRemember) {
            serverPassword.ask(asked, message, user, domain, needsUser, needsDomain, anonymousAllowed, canRemember)
        }
        function onQuestionAsked(asked, message, choices) { serverQuestion.ask(asked, message, choices) }
    }
    FormatDialog {
        id: formatDialog
        types: window.disks ? window.disks.formattable() : []
        labelError: (type, label) => window.disks ? window.disks.labelError(type, label) : ""
        onChosen: (volume, type, label, quick) => window.disks.format(volume, type, label, quick)
    }
    NameDialog {
        id: renameVolume
        title: qsTr("Rename Drive")
        actionText: qsTr("Rename")
        onGiven: (volume, name) => window.disks.rename(volume, name)
    }
    Connections {
        target: window.disks
        enabled: window.foremost
        function onMounted(volume, path) {
            if (volume === window.pendingVolume || window.pendingVolume.length > 0) {
                window.pendingVolume = ""
                window.browser.goPath(path)
            }
        }
        function onEjected(name) { notice.show(qsTr("“%1” can be unplugged").arg(name)) }
        function onPassphraseWrong(volume) { unlockDialog.ask(volume, unlockDialog.name, true) }
        function onFailed(message) { notice.show(message) }
    }
    PasswordDialog {
        id: passwordDialog
        onGiven: (archive, folder, password) => window.ops.extract(archive, folder, password)
    }
    Connections {
        target: window.integrations
        function onNotice(message) { notice.show(message) }
    }
    Connections {
        target: window.ops
        enabled: window.foremost
        function onPasswordWanted(archive, folder, incorrect) { passwordDialog.ask(archive, folder, incorrect) }
    }
    Loader {
        id: extractTo
        active: false
        property string archive
        function ask(archive) {
            extractTo.archive = archive
            active = true
            item.currentFolder = "file://" + archive.substring(0, archive.lastIndexOf("/"))
            item.open()
        }
        sourceComponent: FolderDialog {
            title: qsTr("Extract To")
            onAccepted: window.ops.extract(extractTo.archive, decodeURIComponent(selectedFolder.toString().replace(/^file:\/\//, "")))
        }
    }
    // An action queued here opens the jobs list, as legacy's did; one refused says why.
    Connections {
        target: window.actions
        enabled: window.foremost
        function onStarted(job) { jobsIndicator.openList() }
        function onFailed(message) { notice.show(message) }
    }
    ConfirmDialog { id: confirm }
    // 10xer mode's keys, which the views send their keys to first while it is on.
    readonly property var tenxer: tenxerKeys
    TenXer { id: tenxerKeys; window: window; confirm: confirm; compress: compressDialog; openWith: openWithDialog }
    Shortcut {
        sequence: "Ctrl+Shift+M"
        context: Qt.WindowShortcut
        onActivated: AppSettings.tenxer = !AppSettings.tenxer
    }
    CustomizeDialog { id: customizeDialog }
    PropertiesDialog {
        id: propertiesDialog
        properties: window.properties
        favorites: window.favorites
        onFailed: (message) => notice.show(message)
    }
    OpenWithDialog {
        id: openWithDialog
        apps: window.apps
        onFailed: (message) => notice.show(message)
    }
    SettingsDialog {
        id: settingsDialog
        updates: window.updates
        places: window.places
        theme: SystemTheme
        search: window.search
        actions: window.actions
        currentFolder: window.inFolder ? window.browser.location : ""
        integrations: window.integrations
    }

    // A drop taken in this window: asked about when it crosses drives (as Settings says), and the
    // folder shown after it when Settings says so.
    function dropped(result, target) {
        if (result === Transfers.Asking) {
            dropMenu.target = target
            dropMenu.popup()
        } else {
            afterDrop(target)
        }
    }
    function afterDrop(target) {
        if (window.disks && target.length > 0)
            window.disks.remember(target)
        if (AppSettings.openAfterDrop && target.length > 0 && (window.page !== 0 || target !== window.browser.location))
            window.browser.goPath(target)
    }
    ContextMenu {
        id: dropMenu
        objectName: "dropMenu"
        property string target
        property bool answered
        onAboutToShow: answered = false
        MenuItem { text: qsTr("Copy Here"); onTriggered: { dropMenu.answered = true; window.transfers.answer(true, false); window.afterDrop(dropMenu.target) } }
        MenuItem { text: qsTr("Move Here"); onTriggered: { dropMenu.answered = true; window.transfers.answer(false, true); window.afterDrop(dropMenu.target) } }
        MenuSeparator {}
        MenuItem { text: qsTr("Cancel") }
        onClosed: if (!answered) window.transfers.answer(false, false)
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
        function show(at, folder, isCrumb) {
            path = at; isFolder = folder; crumb = isCrumb === true
            pathOpenWith.load([at])
            popup()
        }
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
        OpenWithMenu {
            id: pathOpenWith
            apps: window.apps
            onOthersRequested: (paths) => openWithDialog.ask(paths)
            onFailed: (message) => notice.show(message)
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
        MenuSeparator {}
        MenuItem { text: qsTr("Properties"); onTriggered: propertiesDialog.show(pathMenu.path) }
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
        function show(at) {
            row = at
            const paths = window.folderModel.selectedPaths()
            itemOpenWith.load(paths)
            window.actions.offer(paths)
            sendTo.targets = window.disks ? window.disks.sendTargets() : []
            popup()
        }
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
        MenuItem {
            text: qsTr("Open")
            visible: itemMenu.single && !itemMenu.folder
            height: visible ? implicitHeight : 0
            onTriggered: window.ops.open(itemMenu.path)
        }
        OpenWithMenu {
            id: itemOpenWith
            apps: window.apps
            onOthersRequested: (paths) => openWithDialog.ask(paths)
            onFailed: (message) => notice.show(message)
        }
        // The user's actions: those placed at the top level, then the rest under Actions.
        MenuSeparator { visible: window.actions.offered.length > 0; height: visible ? implicitHeight : 0 }
        Instantiator {
            model: window.topActions
            delegate: MenuItem {
                required property var modelData
                text: modelData.name
                enabled: modelData.available
                onTriggered: window.runAction(modelData, window.folderModel.selectedPaths(), window.browser.location, false)
            }
            onObjectAdded: (index, item) => itemMenu.insertItem(itemMenu.indexOf(itemActionsAt) + index, item)
            onObjectRemoved: (index, item) => itemMenu.removeItem(item)
        }
        MenuItem { id: itemActionsAt; visible: false; height: 0 }
        // Actions, a submenu only while some are offered there.
        Instantiator {
            model: window.submenuActions.length > 0 ? 1 : 0
            delegate: ActionsMenu {
                actions: window.submenuActions
                onChosen: (action) => window.runAction(action, window.folderModel.selectedPaths(), window.browser.location, false)
            }
            onObjectAdded: (index, menu) => itemMenu.insertMenu(itemMenu.indexOf(itemActionsAt), menu)
            onObjectRemoved: (index, menu) => itemMenu.removeMenu(menu)
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Cut"); onTriggered: window.ops.copy(window.folderModel, true) }
        MenuItem { text: qsTr("Copy"); onTriggered: window.ops.copy(window.folderModel, false) }
        MenuItem {
            text: qsTr("Paste Into Folder")
            visible: itemMenu.single && itemMenu.folder
            height: visible ? implicitHeight : 0
            enabled: window.ops.canPaste
            onTriggered: {
                if (window.disks)
                    window.disks.remember(window.folderModel.pathAt(itemMenu.row))
                window.ops.paste(window.folderModel, itemMenu.row)
            }
        }
        // Send to: removable drives, and folders on them used lately.
        Menu {
            id: sendTo
            title: qsTr("Send to")
            property var targets: []
            enabled: targets.length > 0
            Instantiator {
                model: {
                    const items = []
                    for (const target of sendTo.targets) {
                        items.push({ label: target.name, path: target.path })
                        for (const folder of target.folders)
                            items.push({ label: target.name + " › " + folder.slice(target.path.length + 1), path: folder })
                    }
                    return items
                }
                delegate: MenuItem {
                    required property var modelData
                    text: modelData.label
                    onTriggered: {
                        window.disks.remember(modelData.path)
                        window.ops.transferPaths(window.folderModel.selectedPaths(), modelData.path, false)
                    }
                }
                onObjectAdded: (index, item) => sendTo.insertItem(index, item)
                onObjectRemoved: (index, item) => sendTo.removeItem(item)
            }
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Compress…"); onTriggered: compressDialog.ask(window.folderModel.selectedPaths()) }
        MenuItem {
            text: qsTr("Extract Here")
            visible: itemMenu.single && !itemMenu.folder && window.ops.archiveFormat(itemMenu.path) !== ""
            height: visible ? implicitHeight : 0
            onTriggered: window.ops.extract(itemMenu.path)
        }
        MenuItem {
            text: qsTr("Extract To…")
            visible: itemMenu.single && !itemMenu.folder && window.ops.archiveFormat(itemMenu.path) !== ""
            height: visible ? implicitHeight : 0
            onTriggered: extractTo.ask(itemMenu.path)
        }
        MenuItem {
            text: qsTr("Customize…")
            onTriggered: customizeDialog.show(window.folderModel.selectedPaths(), itemMenu.path.substring(itemMenu.path.lastIndexOf("/") + 1), itemMenu.folder)
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
        MenuSeparator { visible: itemMenu.single; height: visible ? implicitHeight : 0 }
        MenuItem {
            text: qsTr("Properties")
            visible: itemMenu.single
            height: visible ? implicitHeight : 0
            onTriggered: propertiesDialog.show(itemMenu.path)
        }
    }

    // A right click on empty space in a folder.
    ContextMenu {
        id: spaceMenu
        // The folder shown is what its actions run on.
        function show() {
            window.actions.offer([window.browser.location])
            popup()
        }
        MenuItem { text: qsTr("New Folder"); onTriggered: window.ops.create(window.folderModel, true) }
        MenuItem { text: qsTr("New File"); onTriggered: window.ops.create(window.folderModel, false) }
        MenuSeparator {}
        MenuItem { text: qsTr("Paste"); enabled: window.ops.canPaste; onTriggered: window.pasteHere() }
        MenuItem { text: qsTr("Select All"); onTriggered: window.folderModel.selectAll() }
        MenuSeparator {}
        MenuItem { text: window.ops.undoLabel.length > 0 ? qsTr("Undo %1").arg(window.ops.undoLabel) : qsTr("Undo"); enabled: window.ops.undoLabel.length > 0; onTriggered: window.ops.undo() }
        MenuItem { text: window.ops.redoLabel.length > 0 ? qsTr("Redo %1").arg(window.ops.redoLabel) : qsTr("Redo"); enabled: window.ops.redoLabel.length > 0; onTriggered: window.ops.redo() }
        MenuSeparator {}
        MenuItem { text: qsTr("Show Hidden Files"); checkable: true; checked: window.folderModel !== null && window.folderModel.showHidden; onTriggered: window.folderModel.showHidden = !window.folderModel.showHidden }
        MenuSeparator { visible: window.actions.offered.length > 0; height: visible ? implicitHeight : 0 }
        Instantiator {
            model: window.topActions
            delegate: MenuItem {
                required property var modelData
                text: modelData.name
                enabled: modelData.available
                onTriggered: window.runAction(modelData, [window.browser.location], window.browser.location, true)
            }
            onObjectAdded: (index, item) => spaceMenu.insertItem(spaceMenu.indexOf(spaceActionsAt) + index, item)
            onObjectRemoved: (index, item) => spaceMenu.removeItem(item)
        }
        MenuItem { id: spaceActionsAt; visible: false; height: 0 }
        Instantiator {
            model: window.submenuActions.length > 0 ? 1 : 0
            delegate: ActionsMenu {
                actions: window.submenuActions
                onChosen: (action) => window.runAction(action, [window.browser.location], window.browser.location, true)
            }
            onObjectAdded: (index, menu) => spaceMenu.insertMenu(spaceMenu.indexOf(spaceActionsAt), menu)
            onObjectRemoved: (index, menu) => spaceMenu.removeMenu(menu)
        }
    }

    // The header's New menu.
    // The views beyond the three in the header.
    ContextMenu {
        id: viewsMenu
        MenuItem { text: qsTr("Compact List (Ctrl+4)"); checkable: true; checked: window.tabBrowser !== null && window.tabBrowser.view === 3; onTriggered: window.tabBrowser.view = 3 }
        MenuItem { text: qsTr("Gallery (Ctrl+5)"); checkable: true; checked: window.tabBrowser !== null && window.tabBrowser.view === 4; onTriggered: window.tabBrowser.view = 4 }
        MenuItem { text: qsTr("Dual Pane (Ctrl+6)"); checkable: true; checked: window.tabBrowser !== null && window.tabBrowser.view === 5; onTriggered: window.tabBrowser.view = 5 }
    }
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
        MenuSeparator {}
        MenuItem { text: qsTr("Larger Icons (Ctrl+=)"); enabled: window.iconsShown && window.tabBrowser.iconSize < 160; onTriggered: window.tabBrowser.zoom(1) }
        MenuItem { text: qsTr("Smaller Icons (Ctrl+-)"); enabled: window.iconsShown && window.tabBrowser.iconSize > 32; onTriggered: window.tabBrowser.zoom(-1) }
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
            place = { key: model.key, name: model.name, section: model.section, path: model.path, hideable: model.hideable,
                      volume: model.volume, locked: model.locked, ejectable: model.ejectable, removable: model.removable, uri: model.uri,
                      cloud: model.cloud, connected: model.connected }
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
            text: placeMenu.place.locked ? qsTr("Unlock…") : qsTr("Mount")
            visible: !!placeMenu.place.volume && placeMenu.place.path.length === 0
            height: visible ? implicitHeight : 0
            onTriggered: window.openVolume(placeMenu.place.volume, placeMenu.place.locked, placeMenu.place.name)
        }
        MenuItem {
            text: qsTr("Unmount")
            visible: !!placeMenu.place.volume && placeMenu.place.path.length > 0
            height: visible ? implicitHeight : 0
            onTriggered: window.disks.unmount(placeMenu.place.volume)
        }
        MenuItem {
            text: qsTr("Eject")
            visible: !!placeMenu.place.volume && placeMenu.place.ejectable === true
            height: visible ? implicitHeight : 0
            onTriggered: window.disks.eject(placeMenu.place.volume)
        }
        MenuItem {
            text: qsTr("Connect")
            visible: !!placeMenu.place.cloud && !placeMenu.place.connected
            height: visible ? implicitHeight : 0
            onTriggered: window.cloud.connectAccount(placeMenu.place.cloud)
        }
        MenuItem {
            text: qsTr("Disconnect")
            visible: !!placeMenu.place.cloud && placeMenu.place.connected === true
            height: visible ? implicitHeight : 0
            onTriggered: window.cloud.disconnectAccount(placeMenu.place.cloud)
        }
        MenuItem {
            text: qsTr("Disconnect")
            visible: !!placeMenu.place.uri
            height: visible ? implicitHeight : 0
            onTriggered: window.network.disconnectFrom(placeMenu.place.uri)
        }
        MenuItem {
            text: qsTr("Connect to Server…")
            visible: placeMenu.place.section === 3 && window.network !== null
            height: visible ? implicitHeight : 0
            onTriggered: connectServer.ask("", "sftp://")
        }
        MenuItem {
            text: qsTr("Format…")
            visible: !!placeMenu.place.volume && placeMenu.place.removable === true && window.disks !== null && window.disks.formattable().length > 0
            height: visible ? implicitHeight : 0
            onTriggered: formatDialog.ask(placeMenu.place.volume, placeMenu.place.name)
        }
        MenuItem {
            text: qsTr("Rename…")
            visible: !!placeMenu.place.volume && !placeMenu.place.locked
            height: visible ? implicitHeight : 0
            onTriggered: renameVolume.ask(placeMenu.place.volume, placeMenu.place.name)
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
    Shortcut { sequences: [StandardKey.Paste]; enabled: window.inFolder; onActivated: window.pasteHere() }
    Shortcut { sequence: "Delete"; enabled: window.selectionCount > 0; onActivated: window.ops.trash(window.folderModel) }
    Shortcut { sequence: "Shift+Delete"; enabled: window.selectionCount > 0; onActivated: window.deleteSelection() }
    Shortcut { sequences: [StandardKey.Undo]; onActivated: window.ops.undo() }
    Shortcut { sequences: [StandardKey.Redo, "Ctrl+Shift+Z"]; onActivated: window.ops.redo() }
    Shortcut { sequence: "Ctrl+Shift+N"; enabled: window.inFolder; onActivated: window.ops.create(window.folderModel, true) }
    Shortcut { sequence: "Space"; enabled: !previewPopup.visible; onActivated: window.previewOpen = !window.previewOpen }
    Shortcut { sequence: "Ctrl+,"; onActivated: settingsDialog.open() }
    Shortcut {
        sequence: "Alt+Return"
        enabled: window.inFolder && window.selectionCount === 1
        onActivated: propertiesDialog.show(window.folderModel.selectedPaths()[0])
    }
    // Shown for Ctrl+K while 10xer hides it.
    property bool searchWanted: false
    Shortcut { sequence: "Ctrl+K"; onActivated: { window.searchWanted = true; searchField.forceActiveFocus(); searchField.selectAll() } }
    Shortcut { sequence: "Ctrl+1"; onActivated: window.tabBrowser.view = 0 }
    Shortcut { sequence: "Ctrl+2"; onActivated: window.tabBrowser.view = 1 }
    Shortcut { sequence: "Ctrl+3"; onActivated: window.tabBrowser.view = 2 }
    Shortcut { sequence: "Ctrl+4"; onActivated: window.tabBrowser.view = 3 }
    // Icons view's icon size, kept with the folder.
    Shortcut { sequences: ["Ctrl+=", "Ctrl++"]; enabled: window.iconsShown; onActivated: window.tabBrowser.zoom(1) }
    Shortcut { sequence: "Ctrl+-"; enabled: window.iconsShown; onActivated: window.tabBrowser.zoom(-1) }
    Shortcut { sequence: "Ctrl+0"; enabled: window.iconsShown; onActivated: window.tabBrowser.iconSize = 64 }
    Shortcut { sequence: "Ctrl+5"; onActivated: window.tabBrowser.view = 4 }
    Shortcut { sequence: "Ctrl+6"; onActivated: window.tabBrowser.view = 5 }
    // Dual Pane: F5 copies the selection to the other pane's folder, F6 moves it there.
    Shortcut {
        sequence: "F5"
        enabled: window.dualPane && window.selectionCount > 0
        onActivated: window.ops.transferPaths(window.folderModel.selectedPaths(),
                                              (window.rightActive ? window.tabBrowser : window.tabBrowser.second).location, false)
    }
    Shortcut {
        sequence: "F6"
        enabled: window.dualPane && window.selectionCount > 0
        onActivated: window.ops.transferPaths(window.folderModel.selectedPaths(),
                                              (window.rightActive ? window.tabBrowser : window.tabBrowser.second).location, true)
    }
}
