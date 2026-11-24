// SPDX-License-Identifier: MIT
import QtCore
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Window

import AscFiles

// The Files window in its current layout: header across the top, the sidebar of places, and the
// rounded content panel holding the tab row and the folder. B0 wires the models into the look;
// navigation history, tabs and the other views come in B1 and B2.
ApplicationWindow {
    id: window

    required property var folderModel
    required property var jobModel
    required property var backend

    width: 1600
    height: 968
    visible: true
    color: Theme.window
    title: folderModel.folderName

    property int selectedRow: -1
    // List view columns that give way as the panel narrows: the gap, then Mode, then Kind.
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    function home() { return StandardPaths.writableLocation(StandardPaths.HomeLocation).toString().replace("file://", "") }

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
                IconButton { id: sidebarToggle; label: qsTr("Sidebar"); checkable: true; checked: true; iconPath: "M4 3h8a1.5 1.5 0 0 1 1.5 1.5v7A1.5 1.5 0 0 1 12 13H4a1.5 1.5 0 0 1-1.5-1.5v-7A1.5 1.5 0 0 1 4 3zM6.5 3v10" }
                IconButton { label: qsTr("Back"); enabled: false; iconPath: "M13 8H3M7 4 3 8l4 4" }
                IconButton { label: qsTr("Forward"); enabled: false; iconPath: "M3 8h10M9 4l4 4-4 4" }
                IconButton { label: qsTr("Up"); iconPath: "M8 13V3M4 7l4-4 4 4"; onClicked: window.folderModel.openParent() }
            }
            Text {
                Layout.leftMargin: window.width >= 1000 ? 28 : 8
                text: window.folderModel.folder
                elide: Text.ElideLeft
                Layout.maximumWidth: 380
                Layout.minimumWidth: 0
                Layout.fillWidth: true
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
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
                spacing: 2

                PlaceItem { Layout.fillWidth: true; text: qsTr("Home"); iconPath: "M2.5 7.5 8 3l5.5 4.5V13h-11z"; selected: window.folderModel.folder === window.home(); onClicked: window.folderModel.openPath(window.home()) }
                SectionHeading { Layout.fillWidth: true; title: qsTr("Folders"); color: Theme.sectionFolders }
                Repeater {
                    model: [
                        { name: qsTr("Desktop"), location: StandardPaths.DesktopLocation, icon: "M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM6 13.5h4M8 11v2.5" },
                        { name: qsTr("Documents"), location: StandardPaths.DocumentsLocation, icon: "M2 4.5h4l1.5 1.5H14v6.5H2zM5 9h6" },
                        { name: qsTr("Downloads"), location: StandardPaths.DownloadLocation, icon: "M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10" },
                        { name: qsTr("Music"), location: StandardPaths.MusicLocation, icon: "M6 12V3.5l7-1.5v8.5M6 12a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM13 10.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0z" },
                        { name: qsTr("Pictures"), location: StandardPaths.PicturesLocation, icon: "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM2.5 12l4-4 3 3 2-2 2.5 2.5" },
                        { name: qsTr("Videos"), location: StandardPaths.MoviesLocation, icon: "M3.5 4h6A1.5 1.5 0 0 1 11 5.5v5A1.5 1.5 0 0 1 9.5 12h-6A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM11 7l3-2v6l-3-2" }
                    ]
                    PlaceItem {
                        required property var modelData
                        readonly property string path: StandardPaths.writableLocation(modelData.location).toString().replace("file://", "")
                        Layout.fillWidth: true
                        text: modelData.name
                        iconPath: modelData.icon
                        selected: window.folderModel.folder === path
                        onClicked: window.folderModel.openPath(path)
                    }
                }
                SectionHeading { Layout.fillWidth: true; title: qsTr("Storage"); color: Theme.sectionStorage }
                PlaceItem { Layout.fillWidth: true; text: qsTr("System"); iconPath: "M3.5 4h9A1.5 1.5 0 0 1 14 5.5v5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM2 8h12M4.5 10h.01"; sectionColor: Theme.sectionStorage; selected: window.folderModel.folder === "/"; onClicked: window.folderModel.openPath("/") }
                Item { Layout.fillHeight: true }
                Text { Layout.leftMargin: 26; Layout.bottomMargin: 18; text: qsTr("F1 Shortcuts"); color: "#c9cbd6"; font.family: Theme.family; font.pointSize: Theme.tiny; font.weight: Font.Bold }
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
                        spacing: 4
                        TabChip { Layout.alignment: Qt.AlignBottom; text: window.folderModel.folderName; current: true }
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
                        model: window.folderModel
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.vertical: ScrollBar {}
                        delegate: FileRow {
                            width: ListView.view.width
                            selected: index === window.selectedRow
                            showGap: window.showGap
                            showMode: window.showMode
                            showKind: window.showKind
                            onClicked: window.selectedRow = index
                            onActivated: if (isFolder) { window.selectedRow = -1; window.folderModel.openRow(index) }
                        }
                        Text {
                            anchors.centerIn: parent
                            visible: window.folderModel.error.length > 0 || (!window.folderModel.loading && list.count === 0)
                            text: window.folderModel.error.length > 0 ? window.folderModel.error : qsTr("This folder is empty")
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

    Shortcut { sequence: "Backspace"; onActivated: window.folderModel.openParent() }
    Shortcut { sequence: "Ctrl+H"; onActivated: window.folderModel.showHidden = !window.folderModel.showHidden }
}
