// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A labelled button on the field surface, with an optional leading icon and menu chevron, as
// the New button is.
AbstractButton {
    id: button

    property string iconPath
    property bool menu: false

    implicitHeight: Theme.controlSize
    implicitWidth: row.implicitWidth + 20
    focusPolicy: Qt.TabFocus
    Accessible.name: text

    background: Rectangle {
        radius: Theme.radius
        color: button.down ? Theme.control : button.hovered ? Qt.lighter(Theme.field, 1.25) : Theme.field
        border.width: button.visualFocus ? 1 : 0
        border.color: Theme.selectionEdge
    }
    contentItem: Item {
        RowLayout {
            id: row
            anchors.centerIn: parent
            spacing: 8
            Icon { visible: button.iconPath.length > 0; path: button.iconPath; color: Theme.text; size: 15 }
            Text {
                text: button.text
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.body
            }
            Icon { visible: button.menu; path: "m4 6 4 4 4-4"; color: Theme.textMute; size: 11; strokeWidth: 1.8 }
        }
    }
}
