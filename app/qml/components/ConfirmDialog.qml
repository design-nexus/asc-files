// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Asks before something that cannot be undone. The confirming button says what it does, in the
// danger colour; Escape or Cancel leaves everything as it was.
Popup {
    id: dialog

    property string title
    property string message
    property string action: qsTr("Delete")
    /// A key that confirms besides Return (10xer's d, so d d trashes); none after closing.
    property int confirmKey: 0
    onClosed: confirmKey = 0
    signal confirmed()

    function ask(title, message, action, onConfirmed) {
        dialog.title = title
        dialog.message = message
        dialog.action = action
        confirmHandler = onConfirmed
        open()
    }
    property var confirmHandler: null

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 420
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: dialog.title
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: dialog.title
            wrapMode: Text.Wrap
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            text: dialog.message
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            AbstractButton {
                id: confirm
                focus: true
                implicitHeight: Theme.controlSize
                implicitWidth: confirmLabel.implicitWidth + 28
                Accessible.name: dialog.action
                background: Rectangle { radius: Theme.radius; color: confirm.down ? Qt.darker(Theme.danger, 1.2) : Theme.danger }
                contentItem: Text {
                    id: confirmLabel
                    text: dialog.action
                    color: Theme.dangerText
                    font.family: Theme.family
                    font.pointSize: Theme.body
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                Keys.onPressed: (event) => {
                    if (dialog.confirmKey !== 0 && event.key === dialog.confirmKey && event.modifiers === Qt.NoModifier) {
                        event.accepted = true
                        confirm.clicked()
                    }
                }
                Keys.onReturnPressed: confirm.clicked()
                Keys.onEnterPressed: confirm.clicked()
                onClicked: {
                    dialog.close()
                    if (dialog.confirmHandler) dialog.confirmHandler()
                    dialog.confirmed()
                }
            }
        }
    }
}
