// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Compress…: the archive's name, its format and, for ZIP and 7z, an optional password, as
// legacy asked. The archive is made beside the first item, numbered if the name is taken.
Popup {
    id: dialog

    property var paths: []
    signal chosen(var paths, string name, string format, string password)

    function ask(paths) {
        dialog.paths = paths
        const first = paths.length > 0 ? paths[0].substring(paths[0].lastIndexOf("/") + 1) : ""
        name.text = paths.length === 1 ? first : qsTr("archive")
        format.value = "zip"
        password.text = ""
        open()
        name.forceActiveFocus()
        name.selectAll()
    }
    function accept() {
        if (name.text.trim().length === 0) return
        dialog.close()
        dialog.chosen(dialog.paths, name.text.trim(), format.value, format.locks ? password.text : "")
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

    component Field: TextField {
        implicitHeight: 30
        leftPadding: 10
        color: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.body
        placeholderTextColor: Theme.textMute
        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: parent.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
    }

    contentItem: ColumnLayout {
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: qsTr("Compress %n items", "", dialog.paths.length)
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Field {
            id: name
            objectName: "archiveName"
            Layout.fillWidth: true
            onAccepted: dialog.accept()
        }
        Segmented {
            id: format
            objectName: "archiveFormat"
            readonly property bool locks: value === "zip" || value === "7z"
            options: [{ label: ".zip", value: "zip" }, { label: ".7z", value: "7z" }, { label: ".tar.gz", value: "tar.gz" }, { label: ".tar", value: "tar" }]
            value: "zip"
            onPicked: (picked) => value = picked
        }
        Field {
            id: password
            objectName: "archivePassword"
            Layout.fillWidth: true
            enabled: format.locks
            echoMode: TextInput.Password
            placeholderText: qsTr("Password (optional)")
            onAccepted: dialog.accept()
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            PrimaryButton { text: qsTr("Compress"); enabled: name.text.trim().length > 0; onClicked: dialog.accept() }
        }
    }
}
