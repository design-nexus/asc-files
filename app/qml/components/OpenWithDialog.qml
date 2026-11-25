// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Every program, to open the selected files with: those that say they open such files first,
// then the others; typing filters them. The one chosen can be made the default for the type.
Popup {
    id: dialog
    objectName: "openWithDialog"

    required property var apps
    property var paths: []
    property var choices: []
    property string chosen
    signal failed(string message)

    function ask(forPaths) {
        paths = forPaths
        const offered = apps.appsFor(forPaths).map(app => Object.assign({ section: qsTr("Opens these files") }, app))
        const others = apps.otherApps(forPaths).map(app => Object.assign({ section: qsTr("Other programs") }, app))
        choices = offered.concat(others)
        chosen = offered.length > 0 ? offered[0].id : ""
        filter.text = ""
        always.checked = false
        open()
        filter.forceActiveFocus()
    }
    function launch() {
        if (chosen.length === 0) return
        if (always.checked) apps.setDefault(chosen, paths)
        const why = apps.launch(chosen, paths)
        close()
        if (why.length > 0) failed(why)
    }
    readonly property var shown: choices.filter(app => filter.text.length === 0 || app.name.toLowerCase().indexOf(filter.text.toLowerCase()) >= 0)
    readonly property string what: paths.length === 1 ? "“" + paths[0].substring(paths[0].lastIndexOf("/") + 1) + "”"
                                                       : qsTr("%n items", "", paths.length)

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(460, parent ? parent.width - 48 : 460)
    height: Math.min(560, parent ? parent.height - 48 : 560)
    modal: true
    focus: true
    padding: 20
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    contentItem: ColumnLayout {
        spacing: 12
        Text {
            Layout.fillWidth: true
            text: qsTr("Open %1 with").arg(dialog.what)
            elide: Text.ElideMiddle
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        TextField {
            id: filter
            Layout.fillWidth: true
            implicitHeight: 32
            leftPadding: 10
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.body
            placeholderText: qsTr("Search programs")
            placeholderTextColor: Theme.textMute
            background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: filter.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
            onTextEdited: if (dialog.shown.length > 0) dialog.chosen = dialog.shown[0].id
            Keys.onDownPressed: list.forceActiveFocus()
            Keys.onReturnPressed: dialog.launch()
        }
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: dialog.shown
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}
            currentIndex: dialog.shown.findIndex(app => app.id === dialog.chosen)
            section.property: "section"
            section.delegate: Text {
                required property string section
                width: ListView.view.width
                topPadding: 8
                bottomPadding: 4
                leftPadding: 8
                text: section
                color: Theme.textMute
                font.family: Theme.family
                font.pointSize: Theme.tiny
                font.weight: Font.DemiBold
            }
            delegate: Rectangle {
                id: row
                required property var modelData
                width: ListView.view.width
                height: 38
                radius: Theme.radius
                color: modelData.id === dialog.chosen ? Theme.selection : hover.hovered ? Theme.hover : "transparent"
                HoverHandler { id: hover }
                TapHandler {
                    onTapped: dialog.chosen = row.modelData.id
                    onDoubleTapped: { dialog.chosen = row.modelData.id; dialog.launch() }
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 8
                    spacing: 10
                    Image {
                        Layout.preferredWidth: 24
                        Layout.preferredHeight: 24
                        sourceSize: Qt.size(24, 24)
                        source: "image://appicon/" + encodeURIComponent(row.modelData.icon || "application-x-executable")
                        asynchronous: true
                    }
                    Text {
                        Layout.fillWidth: true
                        text: row.modelData.isDefault ? qsTr("%1 (default)").arg(row.modelData.name) : row.modelData.name
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.family
                        font.pointSize: Theme.body
                    }
                }
            }
            Keys.onUpPressed: if (currentIndex > 0) dialog.chosen = dialog.shown[currentIndex - 1].id
            Keys.onDownPressed: if (currentIndex < count - 1) dialog.chosen = dialog.shown[currentIndex + 1].id
            Keys.onReturnPressed: dialog.launch()
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            ToggleSwitch { id: always; Accessible.name: alwaysText.text }
            Text {
                id: alwaysText
                Layout.fillWidth: true
                text: qsTr("Always open %1 with it").arg(dialog.apps.typesText(dialog.paths))
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.family: Theme.family
                font.pointSize: Theme.small
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            Button { text: qsTr("Open"); enabled: dialog.chosen.length > 0; onClicked: dialog.launch() }
        }
    }
}
