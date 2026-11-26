// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles
import AscFiles.System

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
    required property bool selected
    /// Its own colour and icon (path data, or `emoji:…`), from Customize; empty when none.
    required property string customColor
    required property string customIcon
    /// On the clipboard: 1 copied, 2 cut.
    required property int clipMark
    /// The columns after Name, `[{key, width}]` in order; the name takes the rest, up to 600 px.
    property var columns: AppSettings.listColumns
    /// Columns that give way as the list narrows, so the name keeps its room.
    property bool showGap: true
    property bool showMode: true
    property bool showKind: true

    /// The name is being edited in place.
    property bool editing: false

    signal activated()
    signal renameAccepted(string name)
    signal renameCancelled()
    /// A click, with the keys held (Ctrl adds or removes, Shift extends).
    signal clicked(int modifiers)

    height: Theme.rowHeight
    radius: Theme.radius
    color: selected ? Theme.selection : hover.hovered ? Theme.hover : "transparent"

    HoverHandler { id: hover }
    TapHandler {
        id: tap
        onTapped: row.clicked(tap.point.modifiers)
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
                border.color: Qt.rgba(Theme.textDim.r, Theme.textDim.g, Theme.textDim.b, 0.28)
                Icon {
                    visible: !row.customIcon.startsWith("emoji:")
                    anchors.centerIn: parent
                    size: 11
                    strokeWidth: 1.6
                    color: row.customColor.length > 0 ? row.customColor : Theme.folderIcon
                    path: row.customIcon.length > 0 ? row.customIcon
                        : row.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
                Text {
                    visible: row.customIcon.startsWith("emoji:")
                    anchors.centerIn: parent
                    text: row.customIcon.substring(6)
                    font.pixelSize: 12
                }
                ClipBadge { mark: row.clipMark; edge: 11; anchors.right: parent.right; anchors.bottom: parent.bottom; anchors.margins: -5 }
            }
            NameEditor {
                id: editor
                visible: row.editing
                Layout.fillWidth: true
                Layout.preferredHeight: 26
                original: row.name
                isFolder: row.isFolder
                onVisibleChanged: if (visible) begin()
                onCommitted: (name) => row.renameAccepted(name)
                onCancelled: row.renameCancelled()
            }
            Text {
                visible: !row.editing
                Layout.fillWidth: true
                text: row.loaded ? row.name : ""
                elide: Text.ElideMiddle
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
        }
        // Mode, Size, Kind and Modified, in the order and widths chosen (Settings keeps them).
        Repeater {
            model: row.columns
            Item {
                id: cell
                required property var modelData
                readonly property string key: modelData.key
                // Size, right-aligned, keeps a gap after it wherever it is.
                readonly property int gap: key === "size" ? (row.showGap ? 40 : 16) : 0
                visible: (key !== "mode" || row.showMode) && (key !== "kind" || row.showKind)
                Layout.preferredWidth: modelData.width + gap
                implicitHeight: label.implicitHeight
                Text {
                    id: label
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.rightMargin: cell.gap
                    anchors.verticalCenter: parent.verticalCenter
                    horizontalAlignment: cell.key === "size" ? Text.AlignRight : Text.AlignLeft
                    text: cell.key === "mode" ? row.modeOctal : cell.key === "size" ? row.sizeText
                        : cell.key === "kind" ? row.kindText : row.modifiedText
                    elide: Text.ElideRight
                    color: Theme.textDim
                    font.family: Theme.family
                    font.pointSize: Theme.body
                    font.features: cell.key === "mode" || cell.key === "size" ? { "tnum": 1 } : {}
                    HoverHandler { id: cellHover; enabled: cell.key === "mode" }
                    ToolTipText { visible: cellHover.hovered && row.modeSymbolic.length > 0; text: row.modeSymbolic }
                }
            }
        }
        Item { Layout.fillWidth: true }
    }
}
