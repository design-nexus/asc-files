// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// The header's search field, with its shortcut shown at the end.
TextField {
    id: field

    property string shortcut: "Ctrl+K"

    Accessible.name: placeholderText
    Accessible.description: qsTr("Shortcut: %1").arg(shortcut)

    implicitHeight: Theme.controlSize
    leftPadding: 36
    rightPadding: hint.width + 12
    color: Theme.text
    placeholderTextColor: Theme.textDim
    selectionColor: Theme.selection
    selectedTextColor: Theme.text
    font.family: Theme.family
    font.pointSize: Theme.body
    verticalAlignment: TextInput.AlignVCenter

    background: Rectangle {
        radius: Theme.radius
        color: Theme.field
        border.width: field.activeFocus ? 1 : 0
        border.color: Theme.selectionEdge

        Icon {
            x: 12
            anchors.verticalCenter: parent.verticalCenter
            path: "M7 2.5a4.5 4.5 0 1 1 0 9 4.5 4.5 0 0 1 0-9zM10.5 10.5l3 3"
            size: 15
            strokeWidth: 1.5
        }
        Rectangle {
            id: hint
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            width: hintText.implicitWidth + 14
            height: 24
            radius: 5
            color: Theme.control
            visible: field.shortcut.length > 0 && !field.activeFocus
            Text {
                id: hintText
                anchors.centerIn: parent
                text: field.shortcut
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: 7.875
            }
        }
    }
}
