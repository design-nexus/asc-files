// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A question a server asks while connecting (an unknown host key, say), with its own answers.
Popup {
    id: dialog

    property int asked: -1
    property string message
    property var choices: []
    signal answered(int asked, int choice)
    signal cancelled(int asked)

    function ask(asked, message, choices) {
        dialog.asked = asked
        dialog.message = message
        dialog.choices = choices
        open()
    }
    onClosed: if (asked >= 0) { const was = asked; asked = -1; cancelled(was) }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 460
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: dialog.message.split("\n")[0]
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: dialog.message.split("\n")[0]
            wrapMode: Text.Wrap
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            visible: text.length > 0
            text: dialog.message.split("\n").slice(1).filter(line => line.trim().length > 0).join("\n")
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Flow {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Repeater {
                model: dialog.choices
                delegate: Button {
                    required property int index
                    required property string modelData
                    text: modelData
                    onClicked: {
                        const asked = dialog.asked
                        dialog.asked = -1
                        dialog.close()
                        dialog.answered(asked, index)
                    }
                }
            }
        }
    }
}
