// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// An on/off switch in the accent colour.
AbstractButton {
    id: toggle

    checkable: true
    implicitWidth: 38
    implicitHeight: 22
    focusPolicy: Qt.TabFocus
    Accessible.role: Accessible.CheckBox

    background: Rectangle {
        radius: height / 2
        color: toggle.checked ? Theme.accent : Theme.control
        border.width: toggle.visualFocus ? 2 : 1
        border.color: toggle.visualFocus ? Theme.selectionEdge : Theme.panelEdge
        Behavior on color { ColorAnimation { duration: 120 } }
        Rectangle {
            width: 16
            height: 16
            radius: 8
            y: 3
            x: toggle.checked ? parent.width - width - 3 : 3
            color: toggle.checked ? Theme.accentInk : Theme.textDim
            Behavior on x { NumberAnimation { duration: 120 } }
        }
    }
}
