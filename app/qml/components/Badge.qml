// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles

// A count beside a place. Emphasised, it is a pill in the selection colour, as new downloads are.
Rectangle {
    id: badge

    property string text
    property bool emphasised: false

    implicitWidth: emphasised ? Math.max(18, label.implicitWidth + 12) : label.implicitWidth
    implicitHeight: emphasised ? 24 : label.implicitHeight
    radius: 6
    color: emphasised ? Theme.selection : "transparent"

    Text {
        id: label
        anchors.centerIn: parent
        text: badge.text
        color: badge.emphasised ? Theme.accentText : Theme.textMute
        font.family: Theme.family
        font.pointSize: Theme.small
        font.weight: badge.emphasised ? Font.DemiBold : Font.Normal
    }
}
