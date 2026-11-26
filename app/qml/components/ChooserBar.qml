// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// The file chooser's bar, below the folder, when another program asked Files to choose: the name
// to save under, the caller's choices, the filter, and the buttons that answer.
Rectangle {
    id: bar

    /// The request (a `Portal::ChooserJob`).
    required property var job
    /// The values chosen, by choice id.
    property var values: ({})
    property int filter: job ? job.currentFilter : 0
    readonly property alias name: nameField.text
    signal accepted()
    signal cancelled()

    function setName(name) { nameField.text = name }

    implicitHeight: column.implicitHeight + 24
    color: Theme.panel
    Component.onCompleted: {
        const initial = {}
        for (const choice of job.choices) initial[choice.id] = choice.initial
        values = initial
        if (job.kind === "saveFile") {
            nameField.text = job.currentName
            nameField.forceActiveFocus()
            // The name without its extension is selected, as a rename selects it.
            const dot = nameField.text.lastIndexOf(".")
            nameField.select(0, dot > 0 ? dot : nameField.text.length)
        }
    }

    component Choices: ComboBox {
        id: combo
        implicitHeight: 30
        font.family: Theme.family
        font.pointSize: Theme.body
        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: combo.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
        contentItem: Text {
            leftPadding: 10
            rightPadding: combo.indicator.width + 6
            text: combo.displayText
            font: combo.font
            color: Theme.text
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        indicator: Icon {
            x: combo.width - width - 8
            anchors.verticalCenter: parent.verticalCenter
            path: "m4 6 4 4 4-4"
        }
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

    ColumnLayout {
        id: column
        anchors.fill: parent
        anchors.margins: 12
        spacing: 10
        RowLayout {
            visible: bar.job.kind === "saveFile"
            Layout.fillWidth: true
            spacing: 10
            Text { text: qsTr("Name"); color: Theme.textDim; font.family: Theme.family; font.pointSize: Theme.body }
            Field {
                id: nameField
                objectName: "chooserName"
                Layout.fillWidth: true
                onAccepted: bar.accepted()
            }
        }
        Text {
            visible: bar.job.kind === "saveFiles"
            Layout.fillWidth: true
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
            text: qsTr("Saves %1 here").arg(bar.job.names.join(", "))
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Repeater {
                model: bar.job.choices
                delegate: RowLayout {
                    required property var modelData
                    spacing: 6
                    CheckBox {
                        visible: modelData.options.length === 0
                        text: modelData.label
                        checked: bar.values[modelData.id] === "true"
                        onToggled: { const v = Object.assign({}, bar.values); v[modelData.id] = checked ? "true" : "false"; bar.values = v }
                    }
                    Text {
                        visible: modelData.options.length > 0
                        text: modelData.label
                        color: Theme.textDim
                        font.family: Theme.family
                        font.pointSize: Theme.body
                    }
                    Choices {
                        visible: modelData.options.length > 0
                        model: modelData.options
                        textRole: "label"
                        currentIndex: Math.max(0, modelData.options.findIndex(option => option.id === bar.values[modelData.id]))
                        onActivated: (index) => { const v = Object.assign({}, bar.values); v[modelData.id] = modelData.options[index].id; bar.values = v }
                    }
                }
            }
            Item { Layout.fillWidth: true }
            Choices {
                objectName: "chooserFilter"
                visible: bar.job.filters.length > 0
                model: bar.job.filters
                textRole: "label"
                currentIndex: bar.filter
                implicitWidth: 220
                onActivated: (index) => bar.filter = index
            }
            Button { text: qsTr("Cancel"); onClicked: bar.cancelled() }
            PrimaryButton {
                objectName: "chooserAccept"
                text: bar.job.acceptLabel
                enabled: bar.job.kind !== "saveFile" || nameField.text.length > 0
                onClicked: bar.accepted()
            }
        }
    }
}
