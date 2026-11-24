// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A place in the sidebar: icon in its section's colour, name, an optional count, and for a drive
// a bar showing how full it is.
AbstractButton {
    id: place

    property string iconPath
    property color sectionColor: Theme.sectionFolders
    property bool iconFilled: false
    property string badge
    property bool badgeEmphasised: false
    property bool selected: false
    /// How full the drive is, 0–1; negative for a place that is not a drive.
    property real used: -1

    implicitHeight: Theme.placeHeight + (used >= 0 ? 6 : 0)
    implicitWidth: 200
    focusPolicy: Qt.TabFocus
    Accessible.name: text

    background: Rectangle {
        x: 6
        width: place.width - 12
        height: Theme.placeHeight
        radius: Theme.radius
        color: place.selected ? Theme.selection : place.hovered ? Theme.hover : "transparent"
        border.width: place.selected || place.visualFocus ? 1 : 0
        border.color: Theme.selectionEdge
    }
    contentItem: Item {
        RowLayout {
            x: 20
            width: place.width - 36
            height: Theme.placeHeight
            spacing: 13
            Icon {
                path: place.iconPath
                color: place.sectionColor
                fill: place.iconFilled ? place.sectionColor : "transparent"
            }
            Text {
                Layout.fillWidth: true
                text: place.text
                elide: Text.ElideRight
                color: place.selected ? Theme.text : Theme.textSoft
                font.family: Theme.family
                font.pointSize: Theme.body
            }
            Badge {
                visible: place.badge.length > 0
                text: place.badge
                emphasised: place.badgeEmphasised
            }
        }
        Rectangle {
            visible: place.used >= 0
            x: 49
            y: Theme.placeHeight - 2
            width: place.width - 66
            height: 3
            radius: 1.5
            color: Theme.rule
            Rectangle {
                width: parent.width * Math.max(0, Math.min(1, place.used))
                height: parent.height
                radius: parent.radius
                color: place.sectionColor
            }
        }
    }
}
