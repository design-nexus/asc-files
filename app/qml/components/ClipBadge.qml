// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles

// The mark on an item Files put on the clipboard: a copy badge, or scissors for a cut. It sits
// at the icon's corner, in every view, until the clipboard moves on.
Rectangle {
    id: badge

    /// 0 none, 1 copied, 2 cut (FolderModel's clipMark).
    property int mark: 0
    property int edge: 12

    visible: mark > 0
    width: edge
    height: edge
    radius: edge / 2
    color: Theme.raised
    border.width: 1
    border.color: Theme.accent
    Accessible.role: Accessible.Indicator
    Accessible.name: mark === 2 ? qsTr("Cut") : qsTr("Copied")
    Icon {
        anchors.centerIn: parent
        size: badge.edge - 4
        strokeWidth: 1.4
        color: Theme.accent
        path: badge.mark === 2 ? "M5 4.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM5 11.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM4.5 5.5 13 12M4.5 10.5 13 4"
                              : "M5.5 5.5h7v8h-7zM3.5 10.5v-8h7"
    }
}
