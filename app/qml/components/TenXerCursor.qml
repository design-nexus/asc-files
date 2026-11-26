// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles
import AscFiles.System

// 10xer's keyboard cursor: an outline over the row or tile it is on, apart from the selection's
// fill. Drawn above the items, only while the mode is on.
Rectangle {
    z: 2
    visible: AppSettings.tenxer
    radius: Theme.radius
    color: "transparent"
    border.width: 1.5
    border.color: Theme.accent
}
