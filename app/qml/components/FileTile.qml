// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// One entry in Icons view: a large icon (a thumbnail later) over its name in up to two lines.
Rectangle {
    id: tile

    required property int index
    required property string name
    required property bool isFolder
    required property bool loaded
    required property bool selected
    property int iconSize: 64

    signal activated()
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
            Icon {
                anchors.centerIn: parent
                size: tile.iconSize
                strokeWidth: 0.9
                color: tile.isFolder ? Theme.accent : "#8a93ad"
                fill: tile.isFolder ? Qt.rgba(61 / 255, 138 / 255, 232 / 255, 0.18) : "transparent"
                path: tile.isFolder ? "M1.5 4.2c0-.7.5-1.2 1.2-1.2h3.4l1.5 1.5h5.7c.7 0 1.2.5 1.2 1.2v6.6c0 .7-.5 1.2-1.2 1.2H2.7c-.7 0-1.2-.5-1.2-1.2z"
                                    : "M4 2h5l3 3v9H4zM9 2v3h3"
            }
        }
        Text {
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: tile.loaded ? tile.name : ""
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
