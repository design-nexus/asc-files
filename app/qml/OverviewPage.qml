// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// The Overview: files used lately, pinned folders and favorites, every drive with how full it is,
// what is new in Downloads, and the Trash. Each card leads to its place.
Flickable {
    id: page

    required property var browser
    required property var places
    required property var recent
    required property var favorites
    required property var trash
    required property var downloads
    readonly property string status: ""
    readonly property string currentPath: ""
    /// A file was clicked: the window opens it.
    signal fileActivated(string path)
    /// A right click on an item: the window shows what can be done with it.
    signal menuRequested(string path, bool isFolder)

    function activate(path, isFolder) {
        if (isFolder) browser.goPath(path)
        else fileActivated(path)
    }
    function step(delta) {}

    function focusList() { forceActiveFocus() }

    contentHeight: column.implicitHeight + 32
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    ScrollBar.vertical: ScrollBar {}

    component Card: Rectangle {
        id: card
        property string title
        property string action
        signal actionClicked()
        default property alias content: body.data
        radius: 11
        color: Theme.raised
        implicitHeight: body.implicitHeight + 56
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 12
            RowLayout {
                Text { text: card.title; color: Theme.text; font.family: Theme.family; font.pointSize: Theme.body; font.weight: Font.DemiBold }
                Item { Layout.fillWidth: true }
                Text {
                    visible: card.action.length > 0
                    text: card.action
                    color: Theme.accentText
                    font.family: Theme.family
                    font.pointSize: Theme.small
                    TapHandler { onTapped: card.actionClicked() }
                    HoverHandler { cursorShape: Qt.PointingHandCursor }
                }
            }
            ColumnLayout { id: body; Layout.fillWidth: true; spacing: 2 }
        }
    }

    component Line: Rectangle {
        id: line
        property string iconPath
        property color iconColor: Theme.textDim
        property string label
        property string detail
        /// What the line stands for, for its right-click menu (none when empty).
        property string path
        property bool isFolder: true
        signal clicked()
        Layout.fillWidth: true
        height: 30
        radius: Theme.radius
        color: hover.hovered ? Theme.hover : "transparent"
        HoverHandler { id: hover }
        TapHandler { onTapped: line.clicked() }
        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: if (line.path.length > 0) page.menuRequested(line.path, line.isFolder)
        }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 10
            Icon { path: line.iconPath; color: line.iconColor }
            Text { Layout.fillWidth: true; text: line.label; elide: Text.ElideMiddle; color: Theme.text; font.family: Theme.family; font.pointSize: Theme.body }
            Text { text: line.detail; color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.small; font.features: { "tnum": 1 } }
        }
    }

    readonly property string fileIcon: "M4 2h5l3 3v9H4zM9 2v3h3"
    readonly property string folderIcon: "M2 4.5h4l1.5 1.5H14v6.5H2z"

    GridLayout {
        id: column
        x: 16
        y: 12
        width: page.width - 32
        columns: width >= 900 ? 2 : 1
        columnSpacing: 12
        rowSpacing: 12

        Card {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            title: qsTr("Recent")
            action: qsTr("Show all")
            onActionClicked: page.browser.goPage(2)
            Repeater {
                model: page.recent
                delegate: Line {
                    required property int index
                    required property string name
                    required property bool isFolder
                    required property string whenText
                    visible: index < 8
                    iconPath: isFolder ? page.folderIcon : page.fileIcon
                    label: name
                    detail: whenText
                    path: page.recent.pathAt(index)
                    onClicked: page.activate(path, isFolder)
                }
            }
            Text { visible: page.recent.count === 0; text: qsTr("Files you open show up here"); color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.small }
        }
        Card {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            title: qsTr("Storage")
            Repeater {
                model: page.places
                delegate: ColumnLayout {
                    required property var model
                    visible: model.section === 3 && model.page === 0
                    Layout.fillWidth: true
                    spacing: 4
                    Line {
                        iconPath: model.icon
                        iconColor: Theme.sectionStorage
                        label: model.name
                        detail: model.freeText
                        path: model.path
                        onClicked: page.browser.goPath(model.path)
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        Layout.leftMargin: 34
                        Layout.rightMargin: 8
                        Layout.bottomMargin: 4
                        height: 4
                        radius: 2
                        color: Theme.rule
                        Rectangle { width: parent.width * Math.max(0, model.used); height: 4; radius: 2; color: model.used > 0.9 ? Theme.danger : Theme.sectionStorage }
                    }
                }
            }
        }
        Card {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            title: qsTr("Pinned and favorites")
            Repeater {
                model: page.places
                delegate: Line {
                    required property var model
                    visible: model.section === 2
                    iconPath: model.icon
                    iconColor: Theme.sectionPinned
                    label: model.name
                    path: model.path
                    onClicked: page.browser.goPath(model.path)
                }
            }
            Repeater {
                model: page.favorites
                delegate: Line {
                    required property var model
                    iconPath: "M8 2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.2 4.4 13.1l.7-4L2.2 6.3l4-.6z"
                    iconColor: Theme.accent
                    label: model.name
                    detail: model.folder
                    path: model.path
                    isFolder: model.isFolder
                    onClicked: page.activate(model.path, model.isFolder)
                }
            }
            Text {
                visible: page.favorites.count === 0
                text: qsTr("Star files and folders to keep them at hand")
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
        Card {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            title: qsTr("New in Downloads")
            action: qsTr("Open")
            onActionClicked: page.browser.goPath(page.downloads.folder)
            Repeater {
                model: page.downloads
                delegate: Line {
                    required property int index
                    required property string name
                    required property bool isFolder
                    required property string modifiedText
                    visible: index < 6
                    iconPath: isFolder ? page.folderIcon : page.fileIcon
                    label: name
                    detail: modifiedText
                    path: page.downloads.pathAt(index)
                    onClicked: page.activate(path, isFolder)
                }
            }
        }
        Card {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignTop
            title: qsTr("Trash")
            action: qsTr("Open")
            onActionClicked: page.browser.goPage(4)
            Text {
                text: page.trash.count === 0 ? qsTr("The Trash is empty") : qsTr("%n items", "", page.trash.count)
                color: Theme.textDim
                font.family: Theme.family
                font.pointSize: Theme.body
            }
        }
    }
}
