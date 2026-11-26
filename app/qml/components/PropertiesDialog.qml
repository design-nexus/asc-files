// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Properties of one item, as legacy Files showed them: what it is, where, how big (a folder
// counted as you watch), when it changed, what opens it, and who may read, write and run it.
Popup {
    id: dialog
    objectName: "propertiesDialog"

    /// The window's Properties: what is shown.
    required property var properties
    required property var favorites
    signal failed(string message)

    function show(path) {
        properties.path = ""
        properties.path = path
        open()
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(460, parent ? parent.width - 48 : 460)
    height: Math.min(implicitHeight, parent ? parent.height - 48 : implicitHeight)
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }
    onClosed: properties.path = ""

    Connections {
        target: dialog.properties
        function onFailed(message) { dialog.failed(message) }
    }

    component Detail: RowLayout {
        property string title
        property string value
        visible: value.length > 0
        Layout.fillWidth: true
        spacing: 12
        Text {
            Layout.preferredWidth: 110
            Layout.alignment: Qt.AlignTop
            text: parent.title
            color: Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        Text {
            Layout.fillWidth: true
            text: parent.value
            wrapMode: Text.WrapAnywhere
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.small
            textFormat: Text.PlainText
        }
    }
    component Bit: AbstractButton {
        id: bit
        required property int value
        checkable: true
        checked: (dialog.properties.bits & value) !== 0
        enabled: dialog.properties.canChange
        implicitWidth: label.implicitWidth + 18
        implicitHeight: 26
        Accessible.role: Accessible.CheckBox
        onToggled: dialog.properties.setBit(value, checked)
        background: Rectangle {
            radius: Theme.radius - 2
            color: bit.checked ? Theme.selection : Theme.field
            border.width: 1
            border.color: bit.checked ? Theme.selectionEdge : Theme.panelEdge
            opacity: bit.enabled ? 1 : 0.6
        }
        contentItem: Text {
            id: label
            text: bit.text
            color: bit.checked ? Theme.text : Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }
    component Who: RowLayout {
        property string title
        property string who
        property int shift
        Layout.fillWidth: true
        spacing: 6
        Text {
            Layout.preferredWidth: 110
            text: parent.who.length > 0 ? parent.title + " · " + parent.who : parent.title
            elide: Text.ElideRight
            color: Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        Item { Layout.fillWidth: true }
        Bit { text: qsTr("Read"); value: 4 << parent.shift }
        Bit { text: qsTr("Write"); value: 2 << parent.shift }
        Bit { text: dialog.properties.isFolder ? qsTr("Open") : qsTr("Run"); value: 1 << parent.shift }
    }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: dialog.properties.name
        spacing: 14
        RowLayout {
            Layout.fillWidth: true
            spacing: 14
            Icon {
                path: dialog.properties.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                color: dialog.properties.isFolder ? Theme.accent : Theme.folderIcon
                size: 40
                strokeWidth: 1.1
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Text {
                    Layout.fillWidth: true
                    text: dialog.properties.name
                    wrapMode: Text.WrapAnywhere
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.title
                    font.weight: Font.DemiBold
                    textFormat: Text.PlainText
                }
                Text { text: dialog.properties.kind; color: Theme.textDim; font.family: Theme.family; font.pointSize: Theme.small }
            }
            IconButton {
                // Read again when Favorites change (their count does).
                readonly property bool starred: dialog.favorites.count >= 0 && dialog.favorites.contains(dialog.properties.path)
                label: starred ? qsTr("Remove from Favorites") : qsTr("Add to Favorites")
                checked: starred
                iconPath: "M8 2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.2 4.4 13.1l.7-4L2.2 6.3l4-.6z"
                onClicked: starred ? dialog.favorites.remove(dialog.properties.path) : dialog.favorites.add(dialog.properties.path)
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.rule }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 8
            Detail { title: qsTr("Location"); value: dialog.properties.location }
            Detail { title: qsTr("Link to"); value: dialog.properties.linkTarget }
            Detail { objectName: "sizeDetail"; title: qsTr("Size"); value: dialog.properties.sizeText }
            Detail { title: qsTr("Contains"); value: dialog.properties.containsText }
            Text {
                Layout.fillWidth: true
                visible: text.length > 0
                text: dialog.properties.measureWarning
                wrapMode: Text.Wrap
                color: Theme.warning
                font.family: Theme.family
                font.pointSize: Theme.small
            }
            Detail { title: qsTr("Dimensions"); value: dialog.properties.dimensions }
            Detail { title: qsTr("Modified"); value: dialog.properties.modifiedText }
            Detail { title: qsTr("Accessed"); value: dialog.properties.accessedText }
            Detail { title: qsTr("Opens with"); value: dialog.properties.opensWith }
            Detail { title: qsTr("Hidden"); value: dialog.properties.path.length > 0 ? (dialog.properties.hidden ? qsTr("Yes") : qsTr("No")) : "" }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.rule; visible: !dialog.properties.isLink }
        ColumnLayout {
            Layout.fillWidth: true
            visible: !dialog.properties.isLink
            spacing: 8
            Detail { title: qsTr("Permissions"); value: dialog.properties.modeText }
            Who { title: qsTr("Owner"); who: dialog.properties.owner; shift: 6 }
            Who { title: qsTr("Group"); who: dialog.properties.group; shift: 3 }
            Who { title: qsTr("Others"); shift: 0 }
            RowLayout {
                Layout.fillWidth: true
                visible: !dialog.properties.isFolder
                spacing: 10
                ToggleSwitch {
                    id: runnable
                    enabled: dialog.properties.canChange
                    checked: (dialog.properties.bits & 0o111) !== 0
                    Accessible.name: runnableText.text
                    onToggled: dialog.properties.setRunnable(checked)
                }
                Text {
                    id: runnableText
                    Layout.fillWidth: true
                    text: qsTr("Allow running as a program")
                    color: Theme.textDim
                    font.family: Theme.family
                    font.pointSize: Theme.small
                }
            }
            Text {
                visible: !dialog.properties.canChange && dialog.properties.path.length > 0
                text: qsTr("Only the owner can change these.")
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
        Button { Layout.alignment: Qt.AlignRight; text: qsTr("Close"); onClicked: dialog.close() }
    }
}
