// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// The footer under the files: the folder's count, and in 10xer mode what the keys are doing: a
// message, a range being walked, the chord armed (its second keys listed above it), and the
// 10X pill at the far right.
RowLayout {
    id: bar

    required property var keys
    property string status

    spacing: 8

    component Pill: Rectangle {
        property alias text: label.text
        property bool strong: false
        implicitWidth: label.implicitWidth + 12
        implicitHeight: label.implicitHeight + 4
        radius: 4
        color: Theme.field
        border.width: 1
        border.color: strong ? Theme.accent : Theme.panelEdge
        Text {
            id: label
            anchors.centerIn: parent
            color: parent.strong ? Theme.accent : Theme.text
            font.family: Theme.family
            font.pointSize: Theme.small
            font.weight: Font.DemiBold
        }
    }

    // The prompt: its label, what is typed, and what it says (a reason, "2 of 5").
    Text {
        visible: bar.keys.prompt.length > 0
        text: bar.keys.promptLabel
        color: Theme.accent
        font.family: Theme.monoFamily
        font.pointSize: Theme.small
        font.weight: Font.DemiBold
    }
    TextField {
        id: field
        objectName: "tenxerPrompt"
        visible: bar.keys.prompt.length > 0
        Layout.fillWidth: true
        implicitHeight: 24
        leftPadding: 4
        topPadding: 0
        bottomPadding: 0
        color: Theme.text
        font.family: Theme.monoFamily
        font.pointSize: Theme.small
        echoMode: TextInput.Normal
        background: Item {}
        onTextEdited: bar.keys.edited(text)
        Keys.onReturnPressed: bar.keys.submit(text)
        Keys.onEnterPressed: bar.keys.submit(text)
        Keys.onEscapePressed: bar.keys.cancelPrompt()
        Keys.onUpPressed: bar.keys.choose(-1)
        Keys.onDownPressed: bar.keys.choose(1)
        Keys.onTabPressed: {
            const written = bar.keys.written()
            if (written === null) return
            text = written
            bar.keys.edited(text)
        }
        // Leaving it discards it (a filter keeps what was typed).
        onActiveFocusChanged: if (!activeFocus && bar.keys.prompt.length > 0) Qt.callLater(() => {
            if (!field.activeFocus && bar.keys.prompt.length > 0) bar.keys.closePrompt()
        })
        Connections {
            target: bar.keys
            function onPromptOpened(text, from, to) {
                field.text = text
                field.forceActiveFocus()
                field.select(from, to)
            }
            function onPromptChanged() { if (bar.keys.prompt.length === 0) field.text = "" }
        }
        // The folders it offers, above it; the chosen one marked.
        Rectangle {
            anchors.bottom: parent.top
            anchors.bottomMargin: 10
            x: -8
            visible: parent.visible && bar.keys.candidates.length > 0
            width: Math.min(640, bar.width)
            height: Math.min(candidates.contentHeight, 10 * 26) + 12
            radius: Theme.radius
            color: Theme.raised
            border.width: 1
            border.color: Theme.panelEdge
            ListView {
                id: candidates
                anchors.fill: parent
                anchors.margins: 6
                clip: true
                model: bar.keys.candidates
                currentIndex: bar.keys.chosen
                onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)
                delegate: Rectangle {
                    required property int index
                    required property string modelData
                    width: ListView.view.width
                    height: 26
                    radius: Theme.radius
                    color: index === bar.keys.chosen ? Theme.selection : "transparent"
                    Text {
                        anchors.fill: parent
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        verticalAlignment: Text.AlignVCenter
                        text: parent.modelData
                        elide: Text.ElideLeft
                        color: Theme.text
                        font.family: Theme.monoFamily
                        font.pointSize: Theme.small
                    }
                    TapHandler { onTapped: { bar.keys.chosen = parent.index; bar.keys.submit(field.text) } }
                }
            }
        }
    }
    Text {
        visible: bar.keys.prompt.length > 0 && bar.keys.promptStatus.length > 0
        text: bar.keys.promptStatus
        color: bar.keys.promptError.length > 0 ? Theme.danger : Theme.textDim
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    Text {
        Layout.fillWidth: true
        visible: AppSettings.tenxer && bar.keys.prompt.length === 0
        text: bar.keys.flash
        elide: Text.ElideRight
        color: Theme.textDim
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    Item { Layout.fillWidth: true; visible: !AppSettings.tenxer }
    Pill { visible: AppSettings.tenxer && bar.keys.visualLabel.length > 0; text: bar.keys.visualLabel }
    Pill {
        id: chordPill
        visible: AppSettings.tenxer && bar.keys.chord.length > 0
        text: bar.keys.chord + "-"
        // The chord's second keys, above the pill; it never takes the keyboard.
        Rectangle {
            anchors.bottom: parent.top
            anchors.bottomMargin: 8
            anchors.right: parent.right
            visible: parent.visible && bar.keys.chordOptions.length > 0
            width: options.implicitWidth + 20
            height: options.implicitHeight + 16
            radius: Theme.radius
            color: Theme.raised
            border.width: 1
            border.color: Theme.panelEdge
            Column {
                id: options
                anchors.centerIn: parent
                spacing: 4
                Repeater {
                    model: bar.keys.chordOptions
                    delegate: Row {
                        required property var modelData
                        spacing: 12
                        Text {
                            width: 44
                            text: modelData.key
                            color: Theme.accent
                            font.family: Theme.monoFamily
                            font.pointSize: Theme.small
                        }
                        Text {
                            text: modelData.label
                            color: Theme.text
                            font.family: Theme.family
                            font.pointSize: Theme.small
                        }
                    }
                }
            }
        }
    }
    Text {
        text: bar.status
        color: Theme.textSoft
        font.family: Theme.family
        font.pointSize: Theme.small
    }
    Pill {
        visible: AppSettings.tenxer
        text: "10X"
        strong: true
        Accessible.name: qsTr("10xer mode")
        Accessible.description: qsTr("Experimental feature, under active development")
    }
}
