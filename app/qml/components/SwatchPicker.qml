// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// The theme's swatches to pick from, with "Theme default" first (an empty choice).
Row {
    id: picker

    /// The swatch chosen, by name; empty for the theme's default.
    property string value
    /// The colour "Theme default" stands for.
    property color themeColor: Theme.accent
    signal picked(string swatch)

    spacing: 6

    Repeater {
        model: [""].concat(Theme.swatchNames)
        AbstractButton {
            id: swatch
            required property string modelData
            readonly property bool chosen: modelData === picker.value
            width: 22
            height: 22
            focusPolicy: Qt.TabFocus
            Accessible.name: modelData.length > 0 ? modelData : qsTr("Theme default")
            Accessible.role: Accessible.RadioButton
            Accessible.checked: chosen
            ToolTipText { text: swatch.Accessible.name; visible: swatch.hovered }
            onClicked: if (!chosen) picker.picked(modelData)
            background: Rectangle {
                radius: 11
                color: swatch.modelData.length > 0 ? Theme.swatches[swatch.modelData] : picker.themeColor
                border.width: swatch.chosen || swatch.visualFocus ? 2 : 0
                border.color: Theme.text
                // Theme default: marked with a dot of the panel colour.
                Rectangle {
                    visible: swatch.modelData.length === 0
                    anchors.centerIn: parent
                    width: 6
                    height: 6
                    radius: 3
                    color: Theme.panel
                }
            }
        }
    }
}
