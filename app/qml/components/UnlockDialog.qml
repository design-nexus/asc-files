// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A locked drive's passphrase, asked when opening it, and again when the one given was wrong.
// Nothing keeps it.
Popup {
    id: dialog

    property string volume
    property string name
    property bool incorrect
    signal given(string volume, string passphrase)

    function ask(volume, name, incorrect) {
        dialog.volume = volume
        dialog.name = name
        dialog.incorrect = incorrect
        field.text = ""
        open()
        field.forceActiveFocus()
    }
    function accept() {
        if (field.text.length === 0) return
        dialog.close()
        dialog.given(dialog.volume, field.text)
        field.text = ""
    }

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
        Accessible.name: qsTr("Unlock “%1”").arg(dialog.name)
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: qsTr("Unlock “%1”").arg(dialog.name)
            elide: Text.ElideRight
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: dialog.incorrect ? Theme.danger : Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            text: dialog.incorrect ? qsTr("The passphrase is incorrect. Try again.") : qsTr("This drive is encrypted. Enter its passphrase to open it.")
        }
        TextField {
            id: field
            objectName: "unlockPassphrase"
            Layout.fillWidth: true
            implicitHeight: 30
            leftPadding: 10
            echoMode: TextInput.Password
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.body
            background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: parent.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
            onAccepted: dialog.accept()
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            PrimaryButton { text: qsTr("Unlock"); enabled: field.text.length > 0; onClicked: dialog.accept() }
        }
    }
}
