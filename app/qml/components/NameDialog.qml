// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A new name for something (a drive): the current one to edit, the action to confirm.
Popup {
    id: dialog

    property string title
    property string actionText
    property string key
    signal given(string key, string name)

    function ask(key, current) {
        dialog.key = key
        field.text = current
        open()
        field.forceActiveFocus()
        field.selectAll()
    }
    function accept() {
        if (field.text.trim().length === 0) return
        dialog.close()
        dialog.given(dialog.key, field.text.trim())
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
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: dialog.title
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        TextField {
            id: field
            objectName: "nameField"
            Layout.fillWidth: true
            implicitHeight: 30
            leftPadding: 10
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
            PrimaryButton { text: dialog.actionText; enabled: field.text.trim().length > 0; onClicked: dialog.accept() }
        }
    }
}
