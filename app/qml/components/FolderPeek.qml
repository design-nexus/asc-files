// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qt.labs.folderlistmodel

import AscFiles

// 10xer's folder peek: what a folder holds, shown beside it without going in. It never takes the
// keyboard; i again, Esc or moving the cursor closes it.
Popup {
    id: peek

    property string path
    property bool showHidden: false
    readonly property string name: path.substring(path.lastIndexOf("/") + 1)

    function show(folderPath, at) {
        path = folderPath
        // Beside the item, kept inside the window.
        const overlay = parent
        x = Math.max(8, Math.min(at.x, overlay.width - width - 8))
        y = Math.max(8, Math.min(at.y, overlay.height - height - 8))
        open()
    }

    parent: Overlay.overlay
    width: 320
    height: Math.min(360, header.implicitHeight + entries.contentHeight + 28)
    padding: 10
    focus: false
    modal: false
    closePolicy: Popup.CloseOnPressOutside
    background: Rectangle { radius: 12; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    FolderListModel {
        id: listing
        folder: peek.path.length > 0 ? "file://" + peek.path.split("/").map(encodeURIComponent).join("/") : ""
        showDirsFirst: true
        showHidden: peek.showHidden
        showDotAndDotDot: false
        sortCaseSensitive: false
    }

    contentItem: ColumnLayout {
        spacing: 6
        Accessible.role: Accessible.List
        Accessible.name: peek.name
        RowLayout {
            id: header
            Layout.fillWidth: true
            Text {
                Layout.fillWidth: true
                text: peek.name
                elide: Text.ElideMiddle
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
                font.weight: Font.DemiBold
            }
            Text {
                text: listing.status === FolderListModel.Ready ? qsTr("%n items", "", listing.count) : ""
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
        ListView {
            id: entries
            objectName: "folderPeekEntries"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: listing
            ScrollBar.vertical: ScrollBar {}
            delegate: RowLayout {
                required property string fileName
                required property bool fileIsDir
                width: ListView.view.width
                height: 24
                spacing: 8
                Icon {
                    path: fileIsDir ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                    color: fileIsDir ? Theme.accent : Theme.folderIcon
                    size: 13
                }
                Text {
                    Layout.fillWidth: true
                    text: fileName
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.small
                }
            }
            Text {
                anchors.centerIn: parent
                visible: listing.status === FolderListModel.Ready && listing.count === 0
                text: qsTr("Empty folder")
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
    }
}
