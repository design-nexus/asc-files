// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A folder in List view: the column titles, then its entries. Mode, Kind and the gap before Kind
// give way as the view narrows, so the name keeps its room.
ColumnLayout {
    id: view

    required property var browser
    readonly property var folderModel: browser ? browser.folderModel : null
    property int selectedRow: -1
    readonly property string status: selectedRow >= 0 ? qsTr("%n items", "", list.count) + " · " + qsTr("1 selected")
                                                      : qsTr("%n items", "", list.count)
    readonly property bool showGap: list.width >= 1000
    readonly property bool showMode: list.width >= 720
    readonly property bool showKind: list.width >= 600

    function openSelected() {
        if (selectedRow >= 0 && folderModel.isFolder(selectedRow)) {
            browser.goRow(selectedRow)
        }
    }
    function focusList() { list.forceActiveFocus() }

    spacing: 0
    Connections {
        target: view.browser
        function onLocationChanged() { view.selectedRow = -1 }
    }
    onBrowserChanged: selectedRow = -1

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
                visible: index === 1 ? view.showMode : index === 4 ? view.showKind : true
                Layout.fillWidth: modelData[1] < 0
                Layout.maximumWidth: modelData[1] < 0 ? 600 : index === 3 && !view.showGap ? 16 : modelData[1]
                Layout.preferredWidth: modelData[1] < 0 ? 600 : index === 3 && !view.showGap ? 16 : modelData[1]
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
        model: view.folderModel
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        currentIndex: view.selectedRow
        delegate: FileRow {
            width: ListView.view.width
            selected: index === view.selectedRow
            showGap: view.showGap
            showMode: view.showMode
            showKind: view.showKind
            onClicked: { view.selectedRow = index; list.forceActiveFocus() }
            onActivated: if (isFolder) view.browser.goRow(index)
        }
        Keys.onUpPressed: { view.selectedRow = Math.max(0, view.selectedRow - 1); positionViewAtIndex(view.selectedRow, ListView.Contain) }
        Keys.onDownPressed: { view.selectedRow = Math.min(count - 1, view.selectedRow + 1); positionViewAtIndex(view.selectedRow, ListView.Contain) }
        Keys.onReturnPressed: view.openSelected()
        Keys.onEnterPressed: view.openSelected()
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
