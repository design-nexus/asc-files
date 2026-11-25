// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// An archive's contents in the preview, browsed folder by folder as legacy's were: its path as
// crumbs from "Contents", what the folder holds (folders first, as the reader sorted them), and a
// count. Nothing is extracted; the listing is the sandboxed reader's.
ColumnLayout {
    id: view

    /// The preview's `archive`: `{status, message, files, nodes: [{name, parent, folder, size}]}`.
    required property var listing
    /// The folders open, from the archive itself (node 0) down.
    property var trail: [0]
    readonly property int folder: trail[trail.length - 1]
    readonly property var nodes: listing && listing.nodes ? listing.nodes : []
    // Each node's children, by the node's index: built once per listing.
    readonly property var children: {
        const byParent = {}
        for (let index = 1; index < nodes.length; ++index) {
            const parent = nodes[index].parent
            ;(byParent[parent] = byParent[parent] || []).push(index)
        }
        return byParent
    }
    readonly property var shown: children[folder] || []

    onListingChanged: trail = [0]
    spacing: 6

    function open(index) {
        if (nodes[index] && nodes[index].folder) {
            trail = trail.concat([index])
        }
    }
    function goUp() {
        if (trail.length > 1) {
            trail = trail.slice(0, trail.length - 1)
        }
    }

    // Why there is nothing to browse, when there isn't.
    Text {
        Layout.fillWidth: true
        Layout.margins: 12
        visible: view.listing && view.listing.status !== undefined && view.listing.status !== "open"
        wrapMode: Text.Wrap
        color: Theme.textDim
        font.family: Theme.family
        font.pointSize: Theme.body
        text: {
            switch (view.listing ? view.listing.status : "") {
            case "needsPassword": return qsTr("This archive is locked with a password.")
            case "wrongPassword": return qsTr("The password is incorrect.")
            case "unsupported": return qsTr("This archive format is not supported for preview.")
            case "tooLarge": return qsTr("Archive too large to preview.")
            case "error": return view.listing.message || qsTr("This file is not a valid archive or is damaged.")
            default: return ""
            }
        }
    }

    // Where in the archive: "Contents › docs › deep".
    Flow {
        Layout.fillWidth: true
        Layout.leftMargin: 8
        Layout.rightMargin: 8
        visible: view.nodes.length > 1
        spacing: 2
        Repeater {
            model: view.trail
            delegate: Row {
                required property int index
                required property int modelData
                spacing: 2
                Text {
                    text: "›"
                    visible: index > 0
                    color: Theme.textMute
                    font.family: Theme.family
                    font.pointSize: Theme.small
                    anchors.verticalCenter: parent.verticalCenter
                }
                ToolButton {
                    text: modelData === 0 ? qsTr("Contents") : view.nodes[modelData].name
                    enabled: index < view.trail.length - 1
                    font.family: Theme.family
                    font.pointSize: Theme.small
                    onClicked: view.trail = view.trail.slice(0, index + 1)
                    contentItem: Text {
                        text: parent.text
                        font: parent.font
                        color: parent.enabled ? Theme.textDim : Theme.text
                    }
                    background: Rectangle { radius: Theme.radius; color: parent.hovered && parent.enabled ? Theme.hover : "transparent" }
                }
            }
        }
    }

    ListView {
        id: rows
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: view.nodes.length > 1
        clip: true
        focus: true
        model: view.shown
        keyNavigationEnabled: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        Keys.onReturnPressed: view.open(view.shown[currentIndex])
        Keys.onBackPressed: view.goUp()
        Keys.onPressed: event => {
            if (event.key === Qt.Key_Backspace) {
                view.goUp()
                event.accepted = true
            }
        }
        delegate: ItemDelegate {
            id: row
            required property int index
            required property int modelData
            readonly property var node: view.nodes[modelData]
            width: rows.width
            height: Theme.rowHeight
            highlighted: ListView.isCurrentItem
            onClicked: {
                rows.currentIndex = index
                view.open(modelData)
            }
            background: Rectangle { color: row.highlighted ? Theme.selection : row.hovered ? Theme.hover : "transparent"; radius: Theme.radius }
            contentItem: RowLayout {
                spacing: 8
                Icon {
                    color: row.node.folder ? Theme.folderIcon : Theme.textDim
                    path: row.node.folder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
                Text {
                    Layout.fillWidth: true
                    text: row.node.name
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.body
                }
                Text {
                    visible: !row.node.folder
                    text: Qt.locale().formattedDataSize(row.node.size, 1, Locale.DataSizeSIFormat)
                    color: Theme.textDim
                    font.family: Theme.family
                    font.pointSize: Theme.small
                    font.features: { "tnum": 1 }
                }
            }
        }
    }

    Text {
        Layout.fillWidth: true
        Layout.margins: 8
        visible: view.nodes.length > 1
        color: Theme.textMute
        font.family: Theme.family
        font.pointSize: Theme.small
        text: {
            let files = 0, folders = 0
            for (const index of view.shown) {
                view.nodes[index].folder ? ++folders : ++files
            }
            if (files === 0 && folders === 0) {
                return qsTr("This folder is empty")
            }
            const parts = []
            if (files > 0) {
                parts.push(qsTr("%n file(s)", "", files))
            }
            if (folders > 0) {
                parts.push(qsTr("%n folder(s)", "", folders))
            }
            return parts.join(", ")
        }
    }
}
