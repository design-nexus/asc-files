// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// What an interrupted operation (a crash, an unplugged drive) left beside the folder: originals
// a replace or merge had set aside. Each can be put back under its names; an original whose
// name is taken now comes back numbered, never over what holds it.
Popup {
    id: dialog

    required property var ops
    required property var folderModel
    property var leftovers: []

    function review() {
        ops.reviewLeftovers(folderModel)
    }
    /// Whether this window answers (only the window used last does).
    property bool listening: true
    Connections {
        target: dialog.ops
        enabled: dialog.listening
        function onLeftoversReviewed(leftovers) {
            dialog.leftovers = leftovers
            dialog.open()
        }
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 520
    modal: true
    focus: true
    padding: 22
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: qsTr("Items left by an interrupted operation")
        spacing: 12
        Text {
            text: qsTr("Items left by an interrupted operation")
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: qsTr("A replace or merge here stopped before it finished and set these originals aside. Putting them back never replaces anything: an original whose name is taken comes back numbered.")
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        Repeater {
            model: dialog.leftovers
            Rectangle {
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: entry.implicitHeight + 20
                radius: Theme.radius
                color: Theme.panel
                RowLayout {
                    id: entry
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 10
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 3
                        Text {
                            Layout.fillWidth: true
                            text: modelData.kind === "items" ? modelData.names.slice(0, 4).join(", ") + (modelData.names.length > 4 || modelData.more > 0 ? qsTr(", and more") : "")
                                : modelData.kind === "item" ? qsTr("One item")
                                : modelData.kind === "stillOpen" ? qsTr("Still open in a program")
                                : modelData.kind === "gone" ? qsTr("Already gone") : modelData.reason
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.family
                            font.pointSize: Theme.body
                        }
                        Text {
                            text: modelData.modified > 0 ? qsTr("Set aside %1").arg(new Date(modelData.modified * 1000).toLocaleString(Qt.locale(), Locale.ShortFormat)) : ""
                            color: Theme.textMute
                            font.family: Theme.family
                            font.pointSize: Theme.small
                        }
                    }
                    Button {
                        text: qsTr("Put Back")
                        enabled: modelData.kind === "items" || modelData.kind === "item"
                        onClicked: dialog.ops.recover(dialog.folderModel, modelData.name)
                    }
                }
            }
        }
        Button { Layout.alignment: Qt.AlignRight; text: qsTr("Close"); onClicked: dialog.close() }
    }
}
