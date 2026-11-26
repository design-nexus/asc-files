// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A locked archive's password, asked when extracting it needs one, and again when the one given
// was wrong. The password goes to that extraction only; nothing keeps it.
Popup {
    id: dialog

    property string archive
    property string folder
    property bool incorrect
    signal given(string archive, string folder, string password)

    function ask(archive, folder, incorrect) {
        dialog.archive = archive
        dialog.folder = folder
        dialog.incorrect = incorrect
        password.text = ""
        open()
        password.forceActiveFocus()
    }
    function accept() {
        if (password.text.length === 0) return
        dialog.close()
        dialog.given(dialog.archive, dialog.folder, password.text)
        password.text = ""
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
        Accessible.name: qsTr("Archive password")
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: qsTr("Archive password")
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
            text: dialog.incorrect ? qsTr("The password is incorrect. Try again.")
                                   : qsTr("“%1” is protected by a password.").arg(dialog.archive.substring(dialog.archive.lastIndexOf("/") + 1))
        }
        TextField {
            id: password
            objectName: "archivePasswordAsked"
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
            PrimaryButton { text: qsTr("Extract"); enabled: password.text.length > 0; onClicked: dialog.accept() }
        }
    }
}
