// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Running operations, in the footer: a small bar while any runs, and on a click the list of
// them, each with its progress and a way to stop it, and how finished ones ended.
Item {
    id: indicator

    required property var jobs

    implicitWidth: row.implicitWidth
    implicitHeight: 24
    visible: list.count > 0

    RowLayout {
        id: row
        anchors.verticalCenter: parent.verticalCenter
        spacing: 8
        Text {
            text: indicator.jobs.running > 0 ? qsTr("%n operations running", "", indicator.jobs.running) : qsTr("Operations done")
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        Rectangle {
            visible: indicator.jobs.running > 0
            width: 80; height: 4; radius: 2
            color: Theme.rule
            Rectangle {
                width: parent.width * (list.overall < 0 ? 0.3 : list.overall)
                height: 4; radius: 2
                color: Theme.accent
                SequentialAnimation on x {
                    running: list.overall < 0 && indicator.jobs.running > 0
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
            Text {
                visible: indicator.jobs.running === 0
                Layout.alignment: Qt.AlignRight
                text: qsTr("Clear")
                color: Theme.accentText
                font.family: Theme.family
                font.pointSize: Theme.small
                TapHandler { onTapped: { indicator.jobs.clearFinished(); popup.close() } }
            }
        }
    }
}
