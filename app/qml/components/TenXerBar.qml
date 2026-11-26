// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// The footer under the files: the folder's count, and in 10xer mode what the keys are doing: a
// message, a range being walked, the chord armed (its second keys listed above it), and the
// 10X pill at the far right.
RowLayout {
    id: bar

    required property var keys
    property string status

    spacing: 8

    component Pill: Rectangle {
        property alias text: label.text
        property bool strong: false
        implicitWidth: label.implicitWidth + 12
        implicitHeight: label.implicitHeight + 4
        radius: 4
        color: Theme.field
        border.width: 1
        border.color: strong ? Theme.accent : Theme.panelEdge
        Text {
            id: label
            anchors.centerIn: parent
            color: parent.strong ? Theme.accent : Theme.text
            font.family: Theme.family
            font.pointSize: Theme.small
            font.weight: Font.DemiBold
        }
    }

    Text {
        Layout.fillWidth: true
        visible: AppSettings.tenxer
        text: bar.keys.flash
        elide: Text.ElideRight
        color: Theme.textDim
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    Item { Layout.fillWidth: true; visible: !AppSettings.tenxer }
    Pill { visible: AppSettings.tenxer && bar.keys.visualLabel.length > 0; text: bar.keys.visualLabel }
    Pill {
        id: chordPill
        visible: AppSettings.tenxer && bar.keys.chord.length > 0
        text: bar.keys.chord + "-"
        // The chord's second keys, above the pill; it never takes the keyboard.
        Rectangle {
            anchors.bottom: parent.top
            anchors.bottomMargin: 8
            anchors.right: parent.right
            visible: parent.visible && bar.keys.chordOptions.length > 0
            width: options.implicitWidth + 20
            height: options.implicitHeight + 16
            radius: Theme.radius
            color: Theme.raised
            border.width: 1
            border.color: Theme.panelEdge
            Column {
                id: options
                anchors.centerIn: parent
                spacing: 4
                Repeater {
                    model: bar.keys.chordOptions
                    delegate: Row {
                        required property var modelData
                        spacing: 12
                        Text {
                            width: 44
                            text: modelData.key
                            color: Theme.accent
                            font.family: Theme.monoFamily
                            font.pointSize: Theme.small
                        }
                        Text {
                            text: modelData.label
                            color: Theme.text
                            font.family: Theme.family
                            font.pointSize: Theme.small
                        }
                    }
                }
            }
        }
    }
    Text {
        text: bar.status
        color: Theme.textSoft
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    Pill {
        visible: AppSettings.tenxer
        text: "10X"
        strong: true
        Accessible.name: qsTr("10xer mode")
        Accessible.description: qsTr("Experimental feature, under active development")
    }
}
