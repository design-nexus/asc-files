// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// Customize, as legacy's: the selected folders' or files' own colour (seven, or any) and icon
// (sixteen, or an emoji). Each choice shows at once; Reset takes them away.
Popup {
    id: dialog
    objectName: "customizeDialog"

    property var paths: []
    property string name
    property bool isFolder: true
    // The first item's choices, which the dialog shows.
    property string color
    property string icon

    function show(forPaths, forName, folder) {
        paths = forPaths
        name = forName
        isFolder = folder
        read()
        open()
    }
    function read() {
        color = paths.length > 0 ? Customizations.chosenColor(paths[0]) : ""
        icon = paths.length > 0 ? Customizations.chosenIcon(paths[0]) : ""
        custom.text = color.startsWith("#") ? color : ""
        emoji.text = icon.startsWith("emoji:") ? icon.substring(6) : ""
    }
    readonly property string shownColor: paths.length > 0 && color.length > 0 ? Customizations.colorOf(paths[0]) : ""
    readonly property string shownIcon: icon.length === 0 ? "" : icon.startsWith("emoji:") ? icon
                                         : (Customizations.icons.find(i => i.name === icon) || { path: "" }).path

    function setColor(value) { Customizations.setColor(paths, value); read() }
    function setIcon(value) { Customizations.setIcon(paths, value); read() }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(460, parent ? parent.width - 48 : 460)
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    component Heading: Text {
        color: Theme.textMute
        font.family: Theme.family
        font.pointSize: Theme.tiny
        font.weight: Font.DemiBold
        font.capitalization: Font.AllUppercase
        font.letterSpacing: 0.6
    }
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
        RowLayout {
            spacing: 14
            Item {
                implicitWidth: 48
                implicitHeight: 48
                Icon {
                    visible: !dialog.shownIcon.startsWith("emoji:")
                    anchors.centerIn: parent
                    size: 44
                    strokeWidth: 1
                    color: dialog.shownColor.length > 0 ? dialog.shownColor : dialog.isFolder ? Theme.accent : Theme.folderIcon
                    path: dialog.shownIcon.length > 0 ? dialog.shownIcon
                        : dialog.isFolder ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
                }
                Text {
                    visible: dialog.shownIcon.startsWith("emoji:")
                    anchors.centerIn: parent
                    text: dialog.shownIcon.substring(6)
                    font.pixelSize: 36
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Text {
                    Layout.fillWidth: true
                    text: dialog.paths.length === 1 ? dialog.name : qsTr("%n items", "", dialog.paths.length)
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.title
                    font.weight: Font.DemiBold
                }
                Text {
                    text: dialog.isFolder ? qsTr("Customize Folder") : qsTr("Customize File")
                    color: Theme.textDim
                    font.family: Theme.family
                    font.pointSize: Theme.small
                }
            }
        }

        Heading { text: qsTr("Colour") }
        Flow {
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: Customizations.colors
                AbstractButton {
                    id: swatch
                    required property var modelData
                    readonly property bool chosen: dialog.color === modelData.name
                    width: 26; height: 26
                    Accessible.name: modelData.label
                    ToolTipText { text: swatch.modelData.label; visible: swatch.hovered }
                    onClicked: dialog.setColor(chosen ? "" : modelData.name)
                    background: Rectangle {
                        radius: 13
                        color: swatch.modelData.hex
                        border.width: swatch.chosen || swatch.visualFocus ? 2 : 0
                        border.color: Theme.text
                    }
                }
            }
            Field {
                id: custom
                width: 110
                placeholderText: "#3d8ae8"
                maximumLength: 7
                validator: RegularExpressionValidator { regularExpression: /#?[0-9a-fA-F]{0,6}/ }
                onEditingFinished: {
                    const hex = (text.startsWith("#") ? text : "#" + text).toLowerCase()
                    if (/^#[0-9a-f]{6}$/.test(hex)) dialog.setColor(hex)
                }
            }
        }

        Heading { text: qsTr("Icon") }
        GridLayout {
            Layout.fillWidth: true
            columns: 8
            columnSpacing: 4
            rowSpacing: 4
            Repeater {
                model: Customizations.icons
                IconButton {
                    required property var modelData
                    width: 34; height: 34
                    label: modelData.label
                    iconPath: modelData.path
                    checked: dialog.icon === modelData.name
                    onClicked: dialog.setIcon(checked ? "" : modelData.name)
                }
            }
        }
        RowLayout {
            spacing: 8
            Text { text: qsTr("Or an emoji"); color: Theme.textDim; font.family: Theme.family; font.pointSize: Theme.small }
            Field {
                id: emoji
                Layout.preferredWidth: 80
                placeholderText: "🎵"
                maximumLength: 8
                onEditingFinished: if (text.trim().length > 0) dialog.setIcon("emoji:" + text.trim())
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8
            Button { text: qsTr("Reset"); onClicked: { Customizations.reset(dialog.paths); dialog.read() } }
            Button { text: qsTr("Done"); onClicked: dialog.close() }
        }
    }
}
