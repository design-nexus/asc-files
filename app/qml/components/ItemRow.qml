// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// One entry in a list of items from many folders (Recent, Favorites, the Trash): name, the
// folder it is in, size, kind and the list's own time. An item that is gone is dimmed.
Rectangle {
    id: row

    required property int index
    required property string name
    required property string folder
    required property bool isFolder
    required property bool exists
    required property string sizeText
    required property string kindText
    required property string whenText
    property bool selected: false
    property bool showFolder: true
    property bool showKind: true

    signal activated()
    signal clicked()
    signal menuRequested()

    height: Theme.rowHeight
    radius: Theme.radius
    color: selected ? Theme.selection : hover.hovered ? Theme.hover : "transparent"
    opacity: exists ? 1 : 0.5

    HoverHandler { id: hover }
    TapHandler {
        onTapped: row.clicked()
        onDoubleTapped: row.activated()
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: row.menuRequested()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: 160
            Layout.preferredWidth: 380
            Layout.maximumWidth: 420
            spacing: 12
            Rectangle {
                width: 18; height: 18; radius: 5
                color: "transparent"
                border.width: 1
                border.color: Qt.rgba(Theme.textDim.r, Theme.textDim.g, Theme.textDim.b, 0.28)
                Icon {
                    anchors.centerIn: parent
                    size: 11
                    strokeWidth: 1.6
                    color: Theme.folderIcon
                    path: row.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
            }
            Text {
                Layout.fillWidth: true
                text: row.name
                elide: Text.ElideMiddle
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
        }
        Text {
            visible: row.showFolder
            Layout.fillWidth: true
            Layout.maximumWidth: 300
            Layout.leftMargin: 12
            text: row.folder
            elide: Text.ElideMiddle
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Text {
            Layout.preferredWidth: 91
            horizontalAlignment: Text.AlignRight
            text: row.sizeText
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            font.features: { "tnum": 1 }
        }
        Item { Layout.preferredWidth: 24 }
        Text {
            visible: row.showKind
            Layout.preferredWidth: 119
            text: row.kindText
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Text {
            Layout.preferredWidth: 140
            text: row.whenText
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Item { Layout.fillWidth: true }
    }
}
