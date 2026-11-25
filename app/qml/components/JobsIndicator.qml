// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Running operations and the user's actions, in the footer: a small bar while any runs, and on
// a click the list of them, each with its progress and a way to stop it, and how finished ones
// ended (an action's output, and what it made).
Item {
    id: indicator

    required property var jobs
    /// The actions model: its runs.
    property var actions: null
    readonly property int running: jobs.running + (actions ? actions.running : 0)
    /// An action made `path`: the window shows it.
    signal showRequested(string path)

    function openList() { popup.open() }

    implicitWidth: row.implicitWidth
    implicitHeight: 24
    visible: list.count > 0 || runs.count > 0

    RowLayout {
        id: row
        anchors.verticalCenter: parent.verticalCenter
        spacing: 8
        Text {
            text: indicator.running > 0 ? qsTr("%n operations running", "", indicator.running) : qsTr("Operations done")
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        Rectangle {
            visible: indicator.running > 0
            width: 80; height: 4; radius: 2
            color: Theme.rule
            Rectangle {
                width: parent.width * (list.overall < 0 ? 0.3 : list.overall)
                height: 4; radius: 2
                color: Theme.accent
                SequentialAnimation on x {
                    running: list.overall < 0 && indicator.running > 0
                    loops: Animation.Infinite
                    NumberAnimation { from: 0; to: 56; duration: 900 }
                    NumberAnimation { from: 56; to: 0; duration: 900 }
                }
            }
        }
    }
    TapHandler { onTapped: popup.opened ? popup.close() : popup.open() }
    HoverHandler { cursorShape: Qt.PointingHandCursor }

    Popup {
        id: popup
        y: -height - 8
        x: indicator.width - width
        width: 380
        padding: 10
        background: Rectangle { radius: 12; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }
        contentItem: ColumnLayout {
            spacing: 6
            ListView {
                id: list
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 320)
                clip: true
                spacing: 6
                model: indicator.jobs
                // The first running job's progress, or -1 while unknown.
                property real overall: -1
                delegate: ColumnLayout {
                    required property real job
                    required property string label
                    required property int state
                    required property real progress
                    required property string error
                    required property int failed
                    required property bool cancelled
                    width: ListView.view.width
                    spacing: 4
                    onProgressChanged: if (state !== 4) list.overall = progress
                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: label
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.family
                            font.pointSize: Theme.body
                        }
                        Text {
                            text: state === 4 ? (cancelled ? qsTr("Stopped") : failed > 0 ? qsTr("%n failed", "", failed) : qsTr("Done"))
                                : state === 1 ? qsTr("Waiting") : state === 3 ? qsTr("Finishing writing") : progress >= 0 ? Math.round(progress * 100) + "%" : ""
                            color: failed > 0 ? Theme.danger : Theme.textMute
                            font.family: Theme.family
                            font.pointSize: Theme.small
                        }
                        IconButton {
                            visible: state !== 4
                            width: 24; height: 24
                            label: qsTr("Stop")
                            iconPath: "m4.5 4.5 7 7M11.5 4.5l-7 7"
                            onClicked: indicator.jobs.cancel(job)
                        }
                    }
                    Rectangle {
                        visible: state !== 4
                        Layout.fillWidth: true
                        height: 4; radius: 2
                        color: Theme.rule
                        Rectangle { width: parent.width * Math.max(0, progress); height: 4; radius: 2; color: Theme.accent }
                    }
                    Text {
                        visible: error.length > 0
                        Layout.fillWidth: true
                        text: error
                        wrapMode: Text.Wrap
                        color: Theme.danger
                        font.family: Theme.family
                        font.pointSize: Theme.small
                    }
                }
            }
            // The user's actions.
            ListView {
                id: runs
                objectName: "actionRuns"
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(contentHeight, 360)
                visible: count > 0
                clip: true
                spacing: 8
                model: indicator.actions
                delegate: ColumnLayout {
                    id: run
                    required property real job
                    required property string name
                    required property string status
                    required property bool active
                    required property real fraction
                    required property string step
                    required property string message
                    required property string log
                    required property var created
                    property bool details: false
                    width: ListView.view.width
                    spacing: 4
                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: run.name
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.family
                            font.pointSize: Theme.body
                        }
                        Text {
                            text: run.status === "queued" ? qsTr("Waiting")
                                : run.status === "cancelling" ? qsTr("Stopping")
                                : run.status === "succeeded" ? qsTr("Done")
                                : run.status === "failed" ? qsTr("Failed")
                                : run.status === "cancelled" ? qsTr("Stopped")
                                : run.fraction >= 0 ? Math.round(run.fraction * 100) + "%" : qsTr("Running")
                            color: run.status === "failed" ? Theme.danger : Theme.textMute
                            font.family: Theme.family
                            font.pointSize: Theme.small
                        }
                        IconButton {
                            visible: run.active
                            width: 24; height: 24
                            label: qsTr("Stop")
                            iconPath: "m4.5 4.5 7 7M11.5 4.5l-7 7"
                            onClicked: indicator.actions.cancel(run.job)
                        }
                    }
                    Rectangle {
                        visible: run.active
                        Layout.fillWidth: true
                        height: 4; radius: 2
                        color: Theme.rule
                        Rectangle { width: parent.width * Math.max(0, run.fraction); height: 4; radius: 2; color: Theme.accent }
                    }
                    Text {
                        visible: text.length > 0
                        Layout.fillWidth: true
                        text: run.active ? run.step : run.message
                        wrapMode: Text.Wrap
                        color: run.status === "failed" ? Theme.danger : Theme.textMute
                        font.family: Theme.family
                        font.pointSize: Theme.small
                    }
                    RowLayout {
                        visible: !run.active && (run.log.length > 0 || run.created.length > 0)
                        spacing: 12
                        Text {
                            visible: run.log.length > 0
                            text: run.details ? qsTr("Hide output") : qsTr("Output")
                            color: Theme.accentText
                            font.family: Theme.family
                            font.pointSize: Theme.small
                            TapHandler { onTapped: run.details = !run.details }
                        }
                        Text {
                            visible: run.created.length > 0
                            text: qsTr("Show what it made")
                            color: Theme.accentText
                            font.family: Theme.family
                            font.pointSize: Theme.small
                            TapHandler { onTapped: { indicator.showRequested(run.created[0]); popup.close() } }
                        }
                    }
                    ScrollView {
                        visible: run.details
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.min(160, output.implicitHeight + 8)
                        TextArea {
                            id: output
                            text: run.log
                            readOnly: true
                            selectByMouse: true
                            wrapMode: TextEdit.WrapAnywhere
                            textFormat: TextEdit.PlainText
                            color: Theme.textDim
                            font.family: Theme.monoFamily
                            font.pointSize: Theme.small
                            background: Rectangle { radius: Theme.radius; color: Theme.field }
                        }
                    }
                }
            }
            Text {
                visible: indicator.running === 0
                Layout.alignment: Qt.AlignRight
                text: qsTr("Clear")
                color: Theme.accentText
                font.family: Theme.family
                font.pointSize: Theme.small
                TapHandler {
                    onTapped: {
                        indicator.jobs.clearFinished()
                        if (indicator.actions) indicator.actions.clearFinished()
                        popup.close()
                    }
                }
            }
        }
    }
}
