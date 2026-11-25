// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// What the preview drawer shows, the same in its Popup and Panel styles: the item's name, then
// its picture, page, text or rendered Markdown, then its details.
ColumnLayout {
    id: content

    required property var preview
    property bool wrap: true
    property bool rendered: true
    /// Whether it offers to close (Gallery's doesn't: it is the view).
    property bool closable: true
    signal closeRequested()
    // A solid colour as #rrggbb, as rich text takes one.
    function hex(colour) {
        return "#" + [colour.r, colour.g, colour.b].map(v => Math.round(v * 255).toString(16).padStart(2, "0")).join("")
    }

    spacing: 0

    RowLayout {
        Layout.fillWidth: true
        Layout.preferredHeight: 44
        Layout.leftMargin: 14
        Layout.rightMargin: 6
        spacing: 4
        Text {
            Layout.fillWidth: true
            text: content.preview.name
            elide: Text.ElideMiddle
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.body
            font.weight: Font.DemiBold
        }
        IconButton {
            visible: content.preview.kind === "text"
            width: 28; height: 28
            checked: content.wrap
            label: qsTr("Wrap lines")
            iconPath: "M2.5 4h11M2.5 8h9a2 2 0 0 1 0 4H9M10.5 10.5 9 12l1.5 1.5M2.5 12h4"
            onClicked: content.wrap = !content.wrap
        }
        IconButton {
            visible: content.preview.kind === "document" && content.preview.hasSource
            width: 28; height: 28
            checked: !content.rendered
            label: content.rendered ? qsTr("Show source") : qsTr("Show rendered")
            iconPath: "M5.5 4.5 2 8l3.5 3.5M10.5 4.5 14 8l-3.5 3.5"
            onClicked: content.rendered = !content.rendered
        }
        IconButton {
            visible: content.closable
            width: 28; height: 28
            label: qsTr("Close preview (Space)")
            iconPath: "m4.5 4.5 7 7M11.5 4.5l-7 7"
            onClicked: content.closeRequested()
        }
    }

    // The preview itself.
    Item {
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.leftMargin: 12
        Layout.rightMargin: 12

        Image {
            id: picture
            anchors.fill: parent
            visible: (content.preview.kind === "image" || content.preview.kind === "pdf") && status === Image.Ready
            source: content.preview.imageSource
            fillMode: Image.PreserveAspectFit
            cache: false
            smooth: true
            mipmap: true
        }
        ScrollView {
            anchors.fill: parent
            visible: content.preview.kind === "text" || (content.preview.kind === "document" && content.preview.hasSource && !content.rendered)
            clip: true
            TextArea {
                id: code
                readOnly: true
                selectByMouse: true
                text: content.preview.text
                wrapMode: content.wrap ? TextEdit.WrapAnywhere : TextEdit.NoWrap
                // Coloured as its language reads (Markdown's source as Markdown).
                CodeHighlighter {
                    id: highlighter
                    document: code.textDocument
                    fileName: content.preview.name
                    dark: !Theme.light
                }
                color: Theme.text
                selectionColor: Theme.selection
                font.family: Theme.monoFamily
                font.pointSize: Theme.small
                background: Rectangle { radius: Theme.radius; color: Theme.panel }
            }
        }
        ScrollView {
            anchors.fill: parent
            visible: content.preview.kind === "document" && (content.rendered || !content.preview.hasSource)
            clip: true
            TextArea {
                readOnly: true
                selectByMouse: true
                // The document as the reader made it (in the sandbox): HTML with nothing in it
                // that reaches out. Tables get the theme's rules.
                textFormat: TextEdit.RichText
                // (Qt's rich text takes a table's rules from its attributes, not from CSS.)
                text: "<style>th { background: " + Theme.field + "; }</style>"
                      + content.preview.html.replace(/<table>/g, '<table border="1" cellspacing="0" cellpadding="6" style="border-collapse: collapse;" bordercolor="'
                                                                     + content.hex(Theme.mix(Theme.panel, Theme.text, 0.2)) + '">')
                wrapMode: TextEdit.Wrap
                color: Theme.text
                selectionColor: Theme.selection
                font.family: Theme.family
                font.pointSize: Theme.body
                // Links are not followed from a preview.
                onLinkActivated: {}
                background: Rectangle { radius: Theme.radius; color: Theme.panel }
            }
        }
        // Sound and moving pictures: made only for them, and gone (stopped) when the preview moves
        // on. Loaded by name, so Qt Multimedia loads the first time one is shown, not with Files.
        Loader {
            id: media
            anchors.fill: parent
            active: content.preview.kind === "audio" || content.preview.kind === "video"
            source: active ? Qt.resolvedUrl("MediaView.qml") : ""
            Binding { target: media.item; property: "path"; value: content.preview.path; when: media.status === Loader.Ready }
            Binding { target: media.item; property: "video"; value: content.preview.kind === "video"; when: media.status === Loader.Ready }
        }
        // An archive's contents, browsed without extracting them.
        ArchiveView {
            anchors.fill: parent
            visible: content.preview.kind === "archive" && !content.preview.loading
            listing: content.preview.archive
        }
        Icon {
            anchors.centerIn: parent
            visible: !picture.visible && (content.preview.kind === "folder" || content.preview.kind === "other" ||
                     ((content.preview.kind === "image" || content.preview.kind === "pdf") && !content.preview.loading))
            size: 96
            strokeWidth: 0.8
            color: content.preview.kind === "folder" ? Theme.accent : Theme.folderIcon
            path: content.preview.kind === "folder" ? "M2 4.5h4l1.5 1.5H14v6.5H2z" : "M4 2h5l3 3v9H4zM9 2v3h3"
        }
        BusyIndicator {
            anchors.centerIn: parent
            running: content.preview.loading
            visible: running
        }
    }

    // A PDF's pages.
    RowLayout {
        visible: content.preview.kind === "pdf" && content.preview.pageCount > 1
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 6
        spacing: 6
        IconButton { width: 28; height: 28; label: qsTr("Previous page"); enabled: content.preview.page > 1; iconPath: "M10 3 5 8l5 5"; onClicked: content.preview.goToPage(content.preview.page - 1) }
        Text {
            text: qsTr("Page %1 of %2").arg(content.preview.page).arg(content.preview.pageCount)
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
            font.features: { "tnum": 1 }
        }
        IconButton { width: 28; height: 28; label: qsTr("Next page"); enabled: content.preview.page < content.preview.pageCount; iconPath: "m6 3 5 5-5 5"; onClicked: content.preview.goToPage(content.preview.page + 1) }
    }

    Text {
        visible: content.preview.error.length > 0 || content.preview.truncated
        Layout.fillWidth: true
        Layout.leftMargin: 14
        Layout.rightMargin: 14
        Layout.topMargin: 6
        wrapMode: Text.Wrap
        text: content.preview.error.length > 0 ? content.preview.error : qsTr("Showing the first 512 KB")
        color: content.preview.error.length > 0 ? Theme.danger : Theme.textMute
        font.family: Theme.family
        font.pointSize: Theme.small
    }

    // The details.
    ColumnLayout {
        Layout.fillWidth: true
        Layout.margins: 14
        spacing: 6
        Repeater {
            model: [[qsTr("Kind"), "kind"], [qsTr("Size"), "size"], [qsTr("Items"), "items"], [qsTr("Dimensions"), "dimensions"],
                    [qsTr("Pages"), "pages"], [qsTr("Modified"), "modified"], [qsTr("Where"), "where"]]
            delegate: RowLayout {
                required property var modelData
                readonly property var value: content.preview.details[modelData[1]]
                visible: value !== undefined
                Layout.fillWidth: true
                spacing: 14
                Text {
                    Layout.preferredWidth: 84
                    text: modelData[0]
                    color: Theme.textMute
                    font.family: Theme.family
                    font.pointSize: Theme.small
                }
                Text {
                    Layout.fillWidth: true
                    text: value !== undefined ? String(value) : ""
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.small
                }
            }
        }
    }
}
