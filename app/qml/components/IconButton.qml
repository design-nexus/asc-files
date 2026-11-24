// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// A square, flat button holding one icon. Checked, it takes the selection colour, as the sidebar
// toggle does.
AbstractButton {
    id: button

    property string iconPath
    property string label
    property color iconColor: checked ? Theme.accentText : enabled ? Theme.textDim : Theme.textDisabled

    implicitWidth: Theme.controlSize
    implicitHeight: Theme.controlSize
    focusPolicy: Qt.TabFocus
    Accessible.name: label
    ToolTip.text: label
    ToolTip.visible: hovered && label.length > 0
    ToolTip.delay: 600

    background: Rectangle {
        radius: Theme.radius
        color: button.checked ? Theme.selection : button.down ? Theme.control : button.hovered ? Theme.hover : "transparent"
        border.width: button.visualFocus ? 1 : 0
        border.color: Theme.selectionEdge
    }
    contentItem: Item {
        Icon {
            anchors.centerIn: parent
            path: button.iconPath
            color: button.iconColor
        }
    }
}
