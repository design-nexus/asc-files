// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// One entry in Icons view: a large icon (a thumbnail later) over its name in up to two lines.
Rectangle {
    id: tile

    Accessible.role: Accessible.ListItem
    Accessible.name: loaded ? name : ""
    Accessible.description: isFolder ? qsTr("Folder") : ""
    Accessible.selectable: true
    Accessible.selected: selected

    required property int index
    required property string name
    required property bool isFolder
    required property bool loaded
    required property bool selected
    required property string thumbnail
    /// Its own colour and icon (path data, or `emoji:…`), from Customize; empty when none.
    required property string customColor
    required property string customIcon
    /// On the clipboard: 1 copied, 2 cut.
    required property int clipMark
    readonly property color tint: customColor.length > 0 ? customColor : isFolder ? Theme.accent : Theme.folderIcon
    property int iconSize: 64

    property bool editing: false

    signal activated()
    signal renameAccepted(string name)
    signal renameCancelled()
    signal clicked(int modifiers)

    radius: 11
    color: selected ? Theme.selection : hover.hovered ? Theme.hover : "transparent"
    border.width: selected ? 1 : 0
    border.color: Theme.selectionEdge

    HoverHandler { id: hover }
    TapHandler {
        id: tap
        onTapped: tile.clicked(tap.point.modifiers)
        onDoubleTapped: tile.activated()
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 6
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: tile.iconSize
            Image {
                id: picture
                anchors.centerIn: parent
                width: tile.iconSize + 16
                height: tile.iconSize
                // An icon given in Customize shows in its place.
                visible: status === Image.Ready && tile.customIcon.length === 0
                asynchronous: true
                fillMode: Image.PreserveAspectFit
                sourceSize: Qt.size(128, 128)
                source: tile.thumbnail.length > 0 && AppSettings.showThumbnails ? "image://thumbnail/" + tile.thumbnail : ""
            }
            Icon {
                visible: !picture.visible && !tile.customIcon.startsWith("emoji:")
                anchors.centerIn: parent
                size: tile.iconSize
                strokeWidth: 0.9
                color: tile.tint
                fill: tile.isFolder && tile.customIcon.length === 0 ? Qt.rgba(tile.tint.r, tile.tint.g, tile.tint.b, 0.18) : "transparent"
                path: tile.customIcon.length > 0 ? tile.customIcon
                    : tile.isFolder ? "M1.5 4.2c0-.7.5-1.2 1.2-1.2h3.4l1.5 1.5h5.7c.7 0 1.2.5 1.2 1.2v6.6c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z"
                                    : "M4 2h5l3 3v9H4zM9 2v3h3"
            }
            Text {
                visible: !picture.visible && tile.customIcon.startsWith("emoji:")
                anchors.centerIn: parent
                text: tile.customIcon.substring(6)
                font.pixelSize: tile.iconSize * 0.8
            }
            ClipBadge {
                mark: tile.clipMark
                edge: Math.max(14, tile.iconSize / 4)
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.horizontalCenterOffset: tile.iconSize / 2 - edge / 2
                anchors.bottom: parent.bottom
            }
        }
        NameEditor {
            visible: tile.editing
            Layout.fillWidth: true
            original: tile.name
            isFolder: tile.isFolder
            horizontalAlignment: TextInput.AlignHCenter
            onVisibleChanged: if (visible) begin()
            onCommitted: (name) => tile.renameAccepted(name)
            onCancelled: tile.renameCancelled()
        }
        Text {
            visible: !tile.editing
            Layout.fillWidth: true
            Layout.fillHeight: true
            // 10xer's find marks what matched.
                    readonly property string found: AppSettings.tenxer && tile.Window.window && tile.Window.window.tenxer ? tile.Window.window.tenxer.found : ""
                    textFormat: found.length > 0 ? Text.StyledText : Text.PlainText
                    text: !tile.loaded ? "" : found.length > 0 ? Theme.marked(tile.name, found) : tile.name
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WrapAnywhere
            maximumLineCount: 2
            elide: Text.ElideMiddle
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.small
        }
    }
}
