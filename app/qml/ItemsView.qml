// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

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

    function focusList() { list.forceActiveFocus() }
    function open(row) {
        if (opensFolders && items.isFolderAt(row)) {
            browser.goPath(items.pathAt(row))
        }
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
