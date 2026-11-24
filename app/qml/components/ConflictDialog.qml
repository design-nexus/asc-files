// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// The paste's conflict prompt: one clash at a time, the item already there beside the one
// coming, and what to do: replace it (the one there goes to the Trash), keep both (the new one
// numbered), skip, or for two folders merge them. "Do this for the rest" answers the remaining
// clashes the same way; Cancel pastes nothing.
Popup {
    id: dialog

    required property var ops
    property var conflicts: ops.conflicts
    property int at: 0
    property var choices: []
    property bool forRest: false
    readonly property var current: at < conflicts.length ? conflicts[at] : null

    function choose(choice) {
        const picked = choices.slice()
        const count = forRest ? conflicts.length - at : 1
        for (let i = 0; i < count; ++i) {
            // A folder's Merge cannot apply to a file: those keep both.
            const conflict = conflicts[at + i]
            picked.push(choice === 3 && !conflict.canMerge ? 1 : choice)
        }
        choices = picked
        at += count
        if (at >= conflicts.length) {
            const answered = choices
            close()
            ops.resolve(answered)
        }
    }
    function sizeText(side) {
        return side.isFolder ? qsTr("Folder") : Qt.locale().formattedDataSize(side.size, 1, Locale.DataSizeSIFormat)
    }
    function timeText(side) {
        return new Date(side.modified * 1000).toLocaleString(Qt.locale(), Locale.ShortFormat)
    }

    onConflictsChanged: {
        at = 0
        choices = []
        forRest = false
        if (conflicts.length > 0) open()
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 520
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape
    onClosed: if (at < conflicts.length) ops.resolve([])
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    component Side: Rectangle {
        property string heading
        property var side
        Layout.fillWidth: true
        implicitHeight: sideColumn.implicitHeight + 20
        radius: Theme.radius
        color: Theme.panel
        ColumnLayout {
            id: sideColumn
            anchors.fill: parent
            anchors.margins: 10
            spacing: 4
            Text { text: parent.parent.heading; color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.tiny; font.weight: Font.DemiBold }
            Text { text: parent.parent.side ? dialog.sizeText(parent.parent.side) : ""; color: Theme.text; font.family: Theme.family; font.pointSize: Theme.body }
            Text { text: parent.parent.side ? dialog.timeText(parent.parent.side) : ""; color: Theme.textDim; font.family: Theme.family; font.pointSize: Theme.small }
        }
    }

    contentItem: ColumnLayout {
        spacing: 14
        Text {
            Layout.fillWidth: true
            text: dialog.current ? qsTr("“%1” is already in %2").arg(dialog.current.name).arg(dialog.current.folder) : ""
            wrapMode: Text.Wrap
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            visible: dialog.conflicts.length > 1
            text: qsTr("Clash %1 of %2").arg(dialog.at + 1).arg(dialog.conflicts.length)
            color: Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Side { heading: qsTr("ALREADY THERE"); side: dialog.current ? dialog.current.existing : null }
            Side { heading: qsTr("COMING IN"); side: dialog.current ? dialog.current.incoming : null }
        }
        CheckBox {
            id: rest
            visible: dialog.conflicts.length - dialog.at > 1
            checked: dialog.forRest
            onToggled: dialog.forRest = checked
            text: qsTr("Do this for the remaining %n clashes", "", dialog.conflicts.length - dialog.at - 1)
            contentItem: Text {
                leftPadding: rest.indicator.width + 8
                text: rest.text
                color: Theme.textDim
                font.family: Theme.family
                font.pointSize: Theme.body
                verticalAlignment: Text.AlignVCenter
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            Button { text: qsTr("Skip"); onClicked: dialog.choose(2) }
            Button { text: qsTr("Keep Both"); onClicked: dialog.choose(1) }
            Button { visible: dialog.current !== null && dialog.current.canMerge; text: qsTr("Merge"); onClicked: dialog.choose(3) }
            Button { text: qsTr("Replace"); onClicked: dialog.choose(0) }
        }
    }
}
