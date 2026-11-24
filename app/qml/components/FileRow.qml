// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// One entry in List view: icon and name, then Mode (octal, symbolic on hover), Size (right
// aligned, tabular figures), Kind and Modified. A row whose page has not arrived shows its place.
Rectangle {
    id: row

    required property int index
    required property string name
    required property bool isFolder
    required property string kindText
    required property string sizeText
    required property string modifiedText
    required property string modeOctal
    required property string modeSymbolic
    required property bool loaded
    property bool selected: false
    /// Mode, Size, a gap, Kind and Modified; the name takes the rest, up to 600 px.
    property var columns: [70, 91, 40, 119, 140]
    /// Columns that give way as the list narrows, so the name keeps its room.
    property bool showGap: true
    property bool showMode: true
    property bool showKind: true

    signal activated()
    signal clicked(var mouse)

    height: Theme.rowHeight
    radius: Theme.radius
    color: selected ? Theme.selection : hover.hovered ? Theme.hover : "transparent"

    HoverHandler { id: hover }
    TapHandler {
        onTapped: (point, button) => row.clicked(point)
        onDoubleTapped: row.activated()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.maximumWidth: 600
            Layout.preferredWidth: 600
            Layout.minimumWidth: 160
            spacing: 12
            Rectangle {
                width: 18; height: 18; radius: 5
                color: "transparent"
                border.width: 1
                border.color: Qt.rgba(162 / 255, 165 / 255, 186 / 255, 0.28)
                Icon {
                    anchors.centerIn: parent
                    size: 11
                    strokeWidth: 1.6
                    color: "#8a93ad"
                    path: row.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
            }
            Text {
                Layout.fillWidth: true
                text: row.loaded ? row.name : ""
                elide: Text.ElideMiddle
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
        }
        Text {
            visible: row.showMode
            Layout.preferredWidth: row.columns[0]
            text: row.modeOctal
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            font.features: { "tnum": 1 }
            HoverHandler { id: modeHover }
            ToolTipText { visible: modeHover.hovered && row.modeSymbolic.length > 0; text: row.modeSymbolic }
        }
        Text {
            Layout.preferredWidth: row.columns[1]
            horizontalAlignment: Text.AlignRight
            text: row.sizeText
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            font.features: { "tnum": 1 }
        }
        Item { Layout.preferredWidth: row.showGap ? row.columns[2] : 16 }
        Text {
            visible: row.showKind
            Layout.preferredWidth: row.columns[3]
            text: row.kindText
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Text {
            Layout.preferredWidth: row.columns[4]
            text: row.modifiedText
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Item { Layout.fillWidth: true }
    }
}
