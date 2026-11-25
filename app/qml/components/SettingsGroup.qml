// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// A titled card of settings.
ColumnLayout {
    id: group

    property string title
    default property alias rows: card.data

    Layout.fillWidth: true
    spacing: 8

    Text {
        visible: group.title.length > 0
        Layout.leftMargin: 4
        text: group.title
        color: Theme.textMute
        font.family: Theme.family
        font.pointSize: Theme.tiny
        font.weight: Font.DemiBold
        font.capitalization: Font.AllUppercase
        font.letterSpacing: 0.6
    }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: card.implicitHeight
        radius: Theme.radius + 2
        color: Theme.raised
        border.width: 1
        border.color: Theme.panelEdge
        ColumnLayout {
            id: card
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: 0
        }
    }
}
