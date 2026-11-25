// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// One choice of a few, side by side: `options` is `[{label, value}]`; `value` is the one chosen.
Rectangle {
    id: segmented

    property var options: []
    property var value
    signal picked(var value)

    implicitWidth: row.implicitWidth + 4
    implicitHeight: 30
    radius: Theme.radius
    color: Theme.field
    border.width: 1
    border.color: Theme.panelEdge

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 2
        Repeater {
            model: segmented.options
            AbstractButton {
                id: option
                required property var modelData
                readonly property bool chosen: modelData.value === segmented.value
                implicitHeight: 26
                implicitWidth: label.implicitWidth + 22
                focusPolicy: Qt.TabFocus
                Accessible.name: modelData.label
                Accessible.role: Accessible.RadioButton
                Accessible.checked: chosen
                onClicked: if (!chosen) segmented.picked(modelData.value)
                background: Rectangle {
                    radius: Theme.radius - 2
                    color: option.chosen ? Theme.control : option.hovered ? Theme.hover : "transparent"
                    border.width: option.visualFocus ? 1 : 0
                    border.color: Theme.selectionEdge
                }
                contentItem: Text {
                    id: label
                    text: option.modelData.label
                    color: option.chosen ? Theme.text : Theme.textDim
                    font.family: Theme.family
                    font.pointSize: Theme.small
                    font.weight: option.chosen ? Font.DemiBold : Font.Normal
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
        }
    }
}
