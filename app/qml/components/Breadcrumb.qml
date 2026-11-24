// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// The path in the header: crumbs from Home (or the top) down to the folder, each one a way back
// up. A click beside them, or Ctrl+L, turns it into a field to type a path in; Enter goes there
// and Escape leaves it as it was.
Item {
    id: crumbs

    property var browser
    readonly property bool editing: field.visible

    signal edited()

    function edit() {
        field.text = crumbs.browser ? crumbs.browser.location : ""
        field.visible = true
        field.forceActiveFocus()
        field.selectAll()
    }

    implicitHeight: Theme.controlSize
    implicitWidth: row.implicitWidth

    TapHandler { onTapped: crumbs.edit() }

    // When the crumbs are wider than the room, the newest stay in view and the oldest slide out.
    Item {
        anchors.fill: parent
        clip: true
        visible: !field.visible

        RowLayout {
            id: row
            anchors.verticalCenter: parent.verticalCenter
            x: Math.min(0, crumbs.width - implicitWidth)
            width: implicitWidth
            spacing: 2

            Repeater {
                model: crumbs.browser ? crumbs.browser.crumbs : []
                RowLayout {
                    id: crumb
                    required property var modelData
                    required property int index
                    readonly property bool last: index === (crumbs.browser ? crumbs.browser.crumbs.length - 1 : 0)
                    spacing: 2

                    Icon {
                        visible: crumb.index > 0
                        path: "M3 8h10M9 4l4 4-4 4"
                        size: 12
                        strokeWidth: 1.6
                        Layout.leftMargin: 4
                        Layout.rightMargin: 4
                    }
                    AbstractButton {
                        id: button
                        Accessible.name: crumb.modelData.label
                        implicitHeight: 28
                        implicitWidth: label.implicitWidth + (home.visible ? 23 : 0) + 12
                        Layout.maximumWidth: crumb.last ? 320 : 180
                        onClicked: crumbs.browser.goCrumb(crumb.index)
                        background: Rectangle {
                            radius: 6
                            color: button.hovered ? Theme.hover : "transparent"
                        }
                        contentItem: RowLayout {
                            spacing: 8
                            Icon {
                                id: home
                                visible: crumb.modelData.home
                                path: "M2.5 7.5 8 3l5.5 4.5V13h-11z"
                                size: 15
                                Layout.leftMargin: 6
                            }
                            Text {
                                id: label
                                Layout.fillWidth: true
                                Layout.leftMargin: home.visible ? 0 : 6
                                text: crumb.modelData.label
                                elide: Text.ElideMiddle
                                color: crumb.last ? Theme.text : Theme.textDim
                                font.family: Theme.family
                                font.pointSize: Theme.body
                                font.weight: crumb.last ? Font.Medium : Font.Normal
                            }
                        }
                    }
                }
            }
        }
    }

    TextField {
        id: field
        visible: false
        anchors.fill: parent
        color: Theme.text
        selectionColor: Theme.selection
        selectedTextColor: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.body
        leftPadding: 10
        placeholderText: qsTr("Type a path")
        placeholderTextColor: Theme.textMute
        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: Theme.selectionEdge }
        onAccepted: {
            if (crumbs.browser.goTyped(text)) {
                visible = false
                crumbs.edited()
            }
        }
        onActiveFocusChanged: if (!activeFocus) visible = false
        Keys.onEscapePressed: { visible = false; crumbs.edited() }
    }
}
