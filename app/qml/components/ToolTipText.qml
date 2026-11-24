// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// A small tooltip in the Files look, for text that a hover reveals (a symbolic mode, say).
ToolTip {
    id: tip
    delay: 400
    contentItem: Text {
        text: tip.text
        color: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    background: Rectangle { color: Theme.control; radius: 6; border.width: 1; border.color: Theme.panelEdge }
}
