// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles

// A sidebar section's heading: its title in capitals and a rule beneath, both in the section's
// colour.
Column {
    id: heading

    property string title
    property color color: Theme.sectionFolders

    spacing: 7
    leftPadding: 16
    rightPadding: 12
    topPadding: 14
    bottomPadding: 6

    Text {
        text: heading.title.toUpperCase()
        color: heading.color
        font.family: Theme.family
        font.pointSize: Theme.tiny
        font.weight: Font.Bold
        font.letterSpacing: 0.8
        Accessible.role: Accessible.Heading
    }
    Rectangle {
        width: heading.width - heading.leftPadding - heading.rightPadding
        height: 1
        color: heading.color
        opacity: 0.45
    }
}
