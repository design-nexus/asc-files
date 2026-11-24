// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// A right-click menu in the Files look.
Menu {
    id: menu

    padding: 4
    background: Rectangle {
        implicitWidth: 220
        radius: Theme.radius + 2
        color: Theme.raised
        border.width: 1
        border.color: Theme.panelEdge
    }
    delegate: MenuItem {
        id: entry
        implicitHeight: 30
        leftPadding: 12
        rightPadding: 12
        contentItem: Text {
            text: entry.text
            color: entry.enabled ? Theme.text : Theme.textDisabled
            font.family: Theme.family
            font.pointSize: Theme.body
            verticalAlignment: Text.AlignVCenter
            leftPadding: entry.checkable ? 20 : 0
        }
        indicator: Icon {
            visible: entry.checkable && entry.checked
            x: 10
            anchors.verticalCenter: parent.verticalCenter
            path: "M3.5 8.5 6.5 11.5 12.5 4.5"
            color: Theme.accentText
            size: 14
        }
        arrow: Icon {
            visible: entry.subMenu !== null
            x: entry.width - width - 8
            anchors.verticalCenter: parent.verticalCenter
            path: "m6 4 4 4-4 4"
            size: 12
        }
        background: Rectangle {
            radius: Theme.radius
            color: entry.highlighted ? Theme.selection : "transparent"
        }
    }
}
