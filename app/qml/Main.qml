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
    required property var jobModel
    required property var backend

    readonly property var browser: tabs.current
    readonly property var folderModel: browser ? browser.folderModel : null

    width: 1600
    height: 968
    visible: true
    color: Theme.window
    title: browser ? browser.title : ""

    property int selectedRow: -1
    // List view columns that give way as the panel narrows: the gap, then Mode, then Kind.
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    Connections {
        target: window.browser
        function onLocationChanged() { window.selectedRow = -1 }
    }
    onBrowserChanged: selectedRow = -1

    function openSelected() {
        if (selectedRow >= 0 && folderModel.isFolder(selectedRow)) {
            browser.goRow(selectedRow)
        }
    }

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
                onEdited: list.forceActiveFocus()
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
                    section.property: "section"
                    section.delegate: SectionHeading {
                        required property string section
                        width: ListView.view.width
                        title: section === "0" ? qsTr("Folders") : section === "1" ? qsTr("Pinned") : qsTr("Storage")
                        color: section === "0" ? Theme.sectionFolders : section === "1" ? Theme.sectionPinned : Theme.sectionStorage
                    }
                    delegate: PlaceItem {
                        // Roles are read through `model`: `icon` would clash with the button's own.
                        required property var model
                        width: ListView.view.width
                        text: model.name
                        iconPath: model.icon
                        sectionColor: model.section === 0 ? Theme.sectionFolders : model.section === 1 ? Theme.sectionPinned : Theme.sectionStorage
                        used: model.section === 2 ? model.used : -1
                        badge: model.section === 2 ? model.freeText : ""
                        selected: window.browser !== null && window.browser.location === model.path
                        onClicked: window.browser.goPath(model.path)
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
                    RowLayout {
                        Layout.preferredHeight: 36
                        Layout.leftMargin: 24
                        spacing: 0
                        Repeater {
                            // The name takes what the other columns leave, up to 600 px.
                            model: [[qsTr("Name"), -1, false], [qsTr("Mode"), 70, false], [qsTr("Size"), 91, true], ["", 40, false], [qsTr("Kind"), 119, false], [qsTr("Modified"), 140, false]]
                            Text {
                                required property var modelData
                                required property int index
                                visible: index === 1 ? window.showMode : index === 4 ? window.showKind : true
                                Layout.fillWidth: modelData[1] < 0
                                Layout.maximumWidth: modelData[1] < 0 ? 600 : index === 3 && !window.showGap ? 16 : modelData[1]
                                Layout.preferredWidth: modelData[1] < 0 ? 600 : index === 3 && !window.showGap ? 16 : modelData[1]
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
                        objectName: "fileList"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Layout.leftMargin: 8
                        Layout.rightMargin: 8
                        clip: true
                        reuseItems: true
                        cacheBuffer: 600
                        focus: true
                        model: window.folderModel
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.vertical: ScrollBar {}
                        currentIndex: window.selectedRow
                        delegate: FileRow {
                            width: ListView.view.width
                            selected: index === window.selectedRow
                            showGap: window.showGap
                            showMode: window.showMode
                            showKind: window.showKind
                            onClicked: { window.selectedRow = index; list.forceActiveFocus() }
                            onActivated: if (isFolder) window.browser.goRow(index)
                        }
                        Keys.onUpPressed: { window.selectedRow = Math.max(0, window.selectedRow - 1); positionViewAtIndex(window.selectedRow, ListView.Contain) }
                        Keys.onDownPressed: { window.selectedRow = Math.min(count - 1, window.selectedRow + 1); positionViewAtIndex(window.selectedRow, ListView.Contain) }
                        Keys.onReturnPressed: window.openSelected()
                        Keys.onEnterPressed: window.openSelected()
                        Text {
                            anchors.centerIn: parent
                            visible: window.folderModel !== null && (window.folderModel.error.length > 0 || (!window.folderModel.loading && list.count === 0))
                            text: window.folderModel !== null && window.folderModel.error.length > 0 ? window.folderModel.error : qsTr("This folder is empty")
                            color: Theme.textMute
                            font.family: Theme.family
                            font.pointSize: Theme.body
                        }
                    }
                    Text {
                        Layout.alignment: Qt.AlignRight
                        Layout.rightMargin: 18
                        Layout.bottomMargin: 14
                        Layout.topMargin: 6
                        text: window.selectedRow >= 0
                              ? qsTr("%n items", "", list.count) + " · " + qsTr("1 selected")
                              : qsTr("%n items", "", list.count)
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
}
