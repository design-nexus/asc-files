// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// One column of Columns view: a folder's entries, the one the next column shows marked. Clicking
// a folder opens it in the next column; in the last column, a click selects.
Rectangle {
    id: column

    required property var folderModel
    required property string openName
    required property int index
    property bool last: false
    /// 10xer's cursor in this column, or -1.
    property int cursor: -1
    function show(row) { entries.positionViewAtIndex(row, ListView.Contain) }

    signal openRow(int row)
    signal clickRow(int row, int modifiers)
    signal doubleClickRow(int row)
    signal menuRow(int row)

    width: 250
    color: "transparent"

    Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.rule }

    ListView {
        id: entries
        objectName: "columnEntries"
        anchors.fill: parent
        anchors.margins: 6
        anchors.rightMargin: 7
        clip: true
        reuseItems: true
        interactive: false
        model: column.folderModel
        currentIndex: column.cursor
        highlight: TenXerCursor { visible: column.cursor >= 0 }
        highlightFollowsCurrentItem: true
        highlightMoveDuration: 0
        ScrollBar.vertical: ScrollBar {}
        delegate: Rectangle {
            id: entry
            required property int index
            required property string name
            required property bool isFolder
            required property bool loaded
            required property bool selected
            required property string customColor
            required property string customIcon
            readonly property bool open: column.openName.length > 0 && name === column.openName
            width: ListView.view.width
            height: 32
            radius: Theme.radius
            color: selected || (open && !column.last) ? Theme.selection : hover.hovered ? Theme.hover : "transparent"
            HoverHandler { id: hover }
            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: column.menuRow(entry.index)
            }
            TapHandler {
                id: tap
                onTapped: column.clickRow(entry.index, tap.point.modifiers)
                onDoubleTapped: column.doubleClickRow(entry.index)
            }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                spacing: 9
                Icon {
                    visible: !entry.customIcon.startsWith("emoji:")
                    path: entry.customIcon.length > 0 ? entry.customIcon
                        : entry.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                    color: entry.customColor.length > 0 ? entry.customColor : entry.isFolder ? Theme.accent : Theme.folderIcon
                }
                Text {
                    visible: entry.customIcon.startsWith("emoji:")
                    text: entry.customIcon.substring(6)
                    font.pixelSize: 13
                }
                Text {
                    Layout.fillWidth: true
                    text: entry.loaded ? entry.name : ""
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.body
                }
                Icon { visible: entry.isFolder; path: "m6 4 4 4-4 4"; size: 12; color: Theme.textMute }
            }
        }
    }
}
