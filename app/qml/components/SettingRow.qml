// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// One setting: what it is and what it does, with its control at the end.
Item {
    id: row

    property string title
    property string description
    /// The control, on the right.
    default property alias control: slot.data
    /// Drawn under the row above it.
    property bool divided: true

    Layout.fillWidth: true
    implicitHeight: Math.max(54, grid.implicitHeight + 24)

    Rectangle {
        visible: row.divided
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        height: 1
        color: Theme.rule
    }
    // Side by side; on a narrow row the control goes under the words.
    readonly property bool stacked: width < slot.implicitWidth + 300
    GridLayout {
        id: grid
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        columns: row.stacked ? 1 : 2
        columnSpacing: 16
        rowSpacing: 8
        ColumnLayout {
            id: words
            Layout.fillWidth: true
            Layout.preferredWidth: 0
            spacing: 2
            Text {
                Layout.fillWidth: true
                text: row.title
                wrapMode: Text.Wrap
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
            Text {
                Layout.fillWidth: true
                visible: text.length > 0
                text: row.description
                wrapMode: Text.Wrap
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
        Row {
            id: slot
            Layout.alignment: row.stacked ? Qt.AlignLeft : (Qt.AlignVCenter | Qt.AlignRight)
            spacing: 8
        }
    }
}
