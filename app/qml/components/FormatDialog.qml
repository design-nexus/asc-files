// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Format…: a removable drive erased as FAT32, exFAT or NTFS (those this system can make), with a
// new name; quick, or zeroing it first. The button says what it does, in the danger colour.
Popup {
    id: dialog

    property string volume
    property string name
    /// What this system can format: `vfat`, `exfat`, `ntfs`.
    property var types: []
    /// Why a label won't do for a type, or "".
    property var labelError: (type, label) => ""
    property string type: types.length > 0 ? types[0] : ""
    readonly property string problem: labelError(type, field.text)
    signal chosen(string volume, string type, string label, bool quick)

    function ask(volume, name) {
        dialog.volume = volume
        dialog.name = name
        field.text = name.length <= 11 ? name.toUpperCase() : ""
        quick.checked = true
        open()
        field.forceActiveFocus()
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 440
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: qsTr("Format “%1”?").arg(dialog.name)
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: qsTr("Format “%1”?").arg(dialog.name)
            elide: Text.ElideRight
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            text: qsTr("Everything on this drive will be erased. This cannot be undone.")
        }
        Segmented {
            options: dialog.types.map(type => ({ label: type === "vfat" ? "FAT32" : type === "exfat" ? "exFAT" : "NTFS", value: type }))
            value: dialog.type
            onPicked: (picked) => dialog.type = picked
        }
        TextField {
            id: field
            objectName: "formatLabel"
            Layout.fillWidth: true
            implicitHeight: 30
            leftPadding: 10
            placeholderText: qsTr("Name (optional)")
            placeholderTextColor: Theme.textMute
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.body
            background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: dialog.problem.length > 0 ? Theme.danger : parent.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
        }
        Text {
            visible: dialog.problem.length > 0
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.danger
            font.family: Theme.family
            font.pointSize: Theme.small
            text: dialog.problem
        }
        CheckBox { id: quick; text: qsTr("Quick (don't zero the drive first)"); checked: true }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            AbstractButton {
                id: erase
                enabled: dialog.type.length > 0 && dialog.problem.length === 0
                opacity: enabled ? 1 : 0.5
                implicitHeight: Theme.controlSize
                implicitWidth: eraseLabel.implicitWidth + 28
                Accessible.name: eraseLabel.text
                background: Rectangle { radius: Theme.radius; color: erase.down ? Qt.darker(Theme.danger, 1.2) : Theme.danger }
                contentItem: Text {
                    id: eraseLabel
                    text: qsTr("Erase and Format")
                    color: Theme.dangerText
                    font.family: Theme.family
                    font.pointSize: Theme.body
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                onClicked: {
                    dialog.close()
                    dialog.chosen(dialog.volume, dialog.type, field.text, quick.checked)
                }
            }
        }
    }
}
