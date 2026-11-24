// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A tab in the content panel's tab row; the current one sits on the raised surface with its
// close button.
AbstractButton {
    id: tab

    property bool current: false
    signal closeRequested()

    implicitHeight: 30
    implicitWidth: row.implicitWidth + 24
    checkable: false
    focusPolicy: Qt.TabFocus
    Accessible.role: Accessible.PageTab
    Accessible.name: text

    background: Rectangle {
        radius: Theme.radius
        color: tab.current ? Theme.raised : tab.hovered ? Theme.hover : "transparent"
        border.width: tab.visualFocus ? 1 : 0
        border.color: Theme.selectionEdge
    }
    contentItem: Item {
        RowLayout {
            id: row
            anchors.centerIn: parent
            spacing: 14
            Text {
                text: tab.text
                color: tab.current ? Theme.text : Theme.textSoft
                font.family: Theme.family
                font.pointSize: Theme.body
                font.weight: Font.DemiBold
                elide: Text.ElideMiddle
                Layout.maximumWidth: 220
            }
            Icon {
                visible: tab.current
                path: "m4.5 4.5 7 7M11.5 4.5l-7 7"
                color: Theme.textMute
                size: 12
                TapHandler { onTapped: tab.closeRequested() }
            }
        }
    }
}
