// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// A Dual Pane pane's name: its folder, marked while it is the pane in use (the crumbs, menus
// and F5/F6 go by that one).
Rectangle {
    id: bar

    property var browser: null
    property bool active: false

    Layout.fillWidth: true
    implicitHeight: 30
    color: "transparent"

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        height: 2
        radius: 1
        color: bar.active ? Theme.accent : Theme.rule
    }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 8
        Icon { path: "M2 4.5h4l1.5 1.5H14v6.5H2z"; size: 13; color: bar.active ? Theme.accent : Theme.textMute }
        Text {
            Layout.fillWidth: true
            text: bar.browser ? bar.browser.location : ""
            elide: Text.ElideLeft
            color: bar.active ? Theme.text : Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.small
            font.weight: bar.active ? Font.DemiBold : Font.Normal
        }
    }
}
