// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// A dialog's main button, in the accent colour.
AbstractButton {
    id: button

    implicitHeight: Theme.controlSize
    implicitWidth: label.implicitWidth + 28
    focusPolicy: Qt.StrongFocus
    Accessible.name: text
    opacity: enabled ? 1 : 0.5
    background: Rectangle { radius: Theme.radius; color: button.down ? Qt.darker(Theme.accent, 1.2) : Theme.accent }
    contentItem: Text {
        id: label
        text: button.text
        color: Theme.accentInk
        font.family: Theme.family
        font.pointSize: Theme.body
        font.weight: Font.DemiBold
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
}
