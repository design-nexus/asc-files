// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window

import AscFiles

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
    required property var backend

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
                IconButton { id: sidebarToggle; label: qsTr("Sidebar (Ctrl+B)"); checkable: true; checked: true; iconPath: "M4 3h8a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 12 13H4a1.5 1.5 0 0 1-1.5-1.5v-7A1.5 1.5 0 0 1 4 3zM6.5 3v10" }
                IconButton { label: qsTr("Back (Alt+Left)"); enabled: window.browser && window.browser.canGoBack; iconPath: "M13 8H3M7 4 3 8l4 4"; onClicked: window.browser.back() }
                IconButton { label: qsTr("Forward (Alt+Right)"); enabled: window.browser && window.browser.canGoForward; iconPath: "M3 8h10M9 4l4 4-4 4"; onClicked: window.browser.forward() }
                IconButton { label: qsTr("Up (Alt+Up)"); enabled: window.browser && window.browser.canGoUp; iconPath: "M8 13V3M4 7l4-4 4 4"; onClicked: window.browser.up() }
            }
            Breadcrumb {
                id: breadcrumb
                browser: window.browser
                Layout.leftMargin: window.width >= 1000 ? 22 : 6
                Layout.fillWidth: true
                Layout.minimumWidth: 60
                Layout.maximumWidth: 520
                onEdited: window.view.focusList()
            }
            Item { Layout.fillWidth: true }
            SearchField { Layout.fillWidth: true; Layout.maximumWidth: 352; Layout.minimumWidth: 120; placeholderText: qsTr("Search Home and drives") }
            Item { Layout.fillWidth: true }
            Rectangle {
                color: Theme.field; radius: 9
                implicitWidth: views.implicitWidth + 4; implicitHeight: 34
                Row {
                    id: views
                    anchors.centerIn: parent
                    spacing: 2
                    IconButton { width: 30; height: 30; label: qsTr("List"); checked: true; iconPath: "M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" }
                    IconButton { width: 30; height: 30; label: qsTr("Icons"); iconPath: "M3.5 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM10 2.5h2.5a1 1 0 0 1 1 1V6a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V3.5a1 1 0 0 1 1-1zM3.5 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM10 9h2.5a1 1 0 0 1 1 1v2.5a1 1 0 0 1-1 1H10a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1z" }
                    IconButton { width: 30; height: 30; label: qsTr("Columns"); iconPath: "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM8 3v10" }
                }
            }
            Button { Layout.leftMargin: 12; text: qsTr("New"); menu: true; iconPath: "M2 4.2c0-.6.5-1.2 1.2-1.2h3.3l1.5 1.5h4.8c.7 0 1.2.5 1.2 1.2v6.1c0 .7-.5 1.2-1.2 1.2H3.2c-.7 0-1.2-.5-1.2-1.2zM8 7v4M6 9h4" }
            Rectangle {
                Layout.leftMargin: 12
                color: Theme.field; radius: Theme.radius
                implicitWidth: tools.implicitWidth; implicitHeight: Theme.controlSize
                Row {
                    id: tools
                    IconButton { label: qsTr("Appearance"); iconPath: "M8 2.5a5.5 5.5 0 0 0 0 11c.9 0 1.2-.6 1-1.3-.3-.9.2-1.7 1.2-1.7h1.3a2 2 0 0 0 2-2A5.8 5.8 0 0 0 8 2.5zM5.5 7h.01M8 5h.01" }
                    IconButton { label: qsTr("Settings"); iconPath: "M2.5 5h7M12.5 5h1M2.5 11h1M6.5 11h7M11 3.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3zM5 9.5a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3z" }
                }
            }
            IconButton { Layout.leftMargin: 6; label: qsTr("Close"); iconPath: "m4 4 8 8M12 4l-8 8"; onClicked: window.close() }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Sidebar
            ColumnLayout {
                visible: sidebarToggle.checked
                Layout.preferredWidth: Theme.sidebarWidth
                Layout.minimumWidth: Theme.sidebarWidth
                Layout.maximumWidth: Theme.sidebarWidth
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
                        // Roles are read through `model`: `icon` would clash with the button's own.
                        required property var model
                        // A section's first place carries its heading (the pages at the top have none).
                        readonly property var titles: ({ 1: qsTr("Folders"), 2: qsTr("Pinned"), 3: qsTr("Storage") })
                        readonly property var colors: ({ 1: Theme.sectionFolders, 2: Theme.sectionPinned, 3: Theme.sectionStorage })
                        width: ListView.view.width
                        spacing: 2

                        SectionHeading {
                            visible: model.firstInSection && model.section > 0
                            width: parent.width
                            title: visible ? titles[model.section] : ""
                            color: visible ? colors[model.section] : "transparent"
                        }
                        PlaceItem {
                            width: parent.width
                            text: model.name
                            iconPath: model.icon
                            iconFilled: model.page === 3
                            sectionColor: model.section > 0 ? colors[model.section] : Theme.sectionFolders
                            used: model.section === 3 && model.page === 0 ? model.used : -1
                            badge: model.page === 2 ? (window.recent.count > 0 ? String(window.recent.count) : "")
                                 : model.page === 4 ? (window.trash.count > 0 ? String(window.trash.count) : "")
                                 : model.section === 3 ? model.freeText : ""
                            selected: window.browser !== null && (model.page !== 0 ? window.page === model.page
                                                                                   : window.page === 0 && window.browser.location === model.path)
                            onClicked: model.page !== 0 ? window.browser.goPage(model.page) : window.browser.goPath(model.path)
                        }
                    }
                }
                Text { Layout.leftMargin: 26; Layout.bottomMargin: 18; Layout.topMargin: 8; text: qsTr("F1 Shortcuts"); color: "#c9cbd6"; font.family: Theme.family; font.pointSize: Theme.tiny; font.weight: Font.Bold }
            }

            // Content panel
            Rectangle {
                objectName: "panel"
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.rightMargin: 10
                Layout.bottomMargin: 10
                Layout.leftMargin: sidebarToggle.checked ? 0 : 10
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
                    StackLayout {
                        id: pages
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        currentIndex: window.page

                        FolderView { browser: window.browser }
                        OverviewPage {
                            browser: window.browser
                            places: window.places
                            recent: window.recent
                            favorites: window.favorites
                            trash: window.trash
                            downloads: window.downloads
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.recent
                            whenTitle: qsTr("Used")
                            emptyText: qsTr("Files you open show up here")
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.favorites
                            emptyText: qsTr("Star files and folders to keep them at hand")
                        }
                        ItemsView {
                            browser: window.browser
                            items: window.trash
                            whenTitle: qsTr("Deleted")
                            folderTitle: qsTr("Deleted from")
                            emptyText: qsTr("The Trash is empty")
                            opensFolders: false
                        }
                    }
                    Text {
                        Layout.alignment: Qt.AlignRight
                        Layout.rightMargin: 18
                        Layout.bottomMargin: 14
                        Layout.topMargin: 6
                        text: window.view ? window.view.status : ""
                        color: "#c9cbd6"
                        font.family: Theme.family
                        font.pointSize: Theme.small
                    }
                }
            }
        }
    }

    Shortcut { sequences: ["Alt+Left", "Backspace"]; enabled: !breadcrumb.editing; onActivated: window.browser.back() }
    Shortcut { sequence: "Alt+Right"; onActivated: window.browser.forward() }
    Shortcut { sequence: "Alt+Up"; onActivated: window.browser.up() }
    Shortcut { sequence: "Ctrl+L"; onActivated: breadcrumb.edit() }
    Shortcut { sequence: "Ctrl+T"; onActivated: window.tabs.openTab() }
    Shortcut { sequence: "Ctrl+W"; onActivated: window.tabs.closeTab(window.tabs.currentIndex) }
    Shortcut { sequence: "Ctrl+Tab"; onActivated: window.tabs.next() }
    Shortcut { sequence: "Ctrl+Shift+Tab"; onActivated: window.tabs.previous() }
    Shortcut { sequence: "Ctrl+B"; onActivated: sidebarToggle.checked = !sidebarToggle.checked }
    Shortcut { sequence: "Ctrl+H"; onActivated: window.folderModel.showHidden = !window.folderModel.showHidden }
    Shortcut { sequence: "Ctrl+1"; onActivated: window.browser.goPage(1) }
}
