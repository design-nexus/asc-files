// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles
import AscFiles.System

// Settings: legacy Files' pages, in a card over the window. Every setting applies at once and
// is kept; none needs a restart. Settings of features Files doesn't have yet come with them.
Popup {
    id: dialog
    objectName: "settingsDialog"

    required property var places
    required property var theme
    /// The folder the window shows, offered as the start folder.
    property string currentFolder
    property int page: 0

    readonly property var pages: [
        { title: qsTr("General"), detail: qsTr("Start folder, tabs, dates") },
        { title: qsTr("Browsing"), detail: qsTr("Opening items, views, sorting") },
        { title: qsTr("Sidebar"), detail: qsTr("Places shown, colours") },
        { title: qsTr("Previews"), detail: qsTr("Preview style, thumbnails") },
        { title: qsTr("Appearance"), detail: qsTr("Theme, accent, text size") },
        { title: qsTr("About"), detail: qsTr("Version, diagnostics") }
    ]
    readonly property var viewOptions: [
        { label: qsTr("List"), value: 0 }, { label: qsTr("Icons"), value: 1 }, { label: qsTr("Columns"), value: 2 }
    ]
    readonly property var clickOptions: [{ label: qsTr("Single"), value: 1 }, { label: qsTr("Double"), value: 2 }]

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(880, parent ? parent.width - 48 : 880)
    height: Math.min(640, parent ? parent.height - 48 : 640)
    modal: true
    focus: true
    padding: 0
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: Theme.panelRadius; color: Theme.panel; border.width: 1; border.color: Theme.panelEdge }

    contentItem: RowLayout {
        spacing: 0

        // Pages
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: dialog.width >= 720 ? 230 : 170
            color: Theme.window
            radius: Theme.panelRadius
            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 12
                spacing: 2
                Text {
                    Layout.leftMargin: 10
                    Layout.topMargin: 6
                    Layout.bottomMargin: 10
                    text: qsTr("Settings")
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.title
                    font.weight: Font.DemiBold
                }
                Repeater {
                    model: dialog.pages
                    AbstractButton {
                        id: pageButton
                        required property int index
                        required property var modelData
                        objectName: "settingsPage" + index
                        Layout.fillWidth: true
                        implicitHeight: 48
                        Accessible.name: modelData.title
                        onClicked: dialog.page = index
                        background: Rectangle {
                            radius: Theme.radius
                            color: dialog.page === pageButton.index ? Theme.selection : pageButton.hovered ? Theme.hover : "transparent"
                        }
                        contentItem: ColumnLayout {
                            spacing: 1
                            Text {
                                Layout.leftMargin: 10
                                text: pageButton.modelData.title
                                color: Theme.text
                                font.family: Theme.family
                                font.pointSize: Theme.body
                                font.weight: dialog.page === pageButton.index ? Font.DemiBold : Font.Normal
                            }
                            Text {
                                Layout.leftMargin: 10
                                Layout.fillWidth: true
                                text: pageButton.modelData.detail
                                elide: Text.ElideRight
                                color: Theme.textMute
                                font.family: Theme.family
                                font.pointSize: Theme.small
                            }
                        }
                    }
                }
                Item { Layout.fillHeight: true }
            }
        }

        // The page: the width left beside the pages, whatever its rows would like.
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredWidth: 0
            Layout.minimumWidth: 0
            spacing: 0
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 16
                Layout.leftMargin: 28
                Text {
                    Layout.fillWidth: true
                    text: dialog.pages[dialog.page].title
                    color: Theme.text
                    font.family: Theme.family
                    font.pointSize: Theme.title
                    font.weight: Font.DemiBold
                }
                IconButton { label: qsTr("Close"); iconPath: "m4 4 8 8M12 4l-8 8"; onClicked: dialog.close() }
            }
            ScrollView {
                id: scroll
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredWidth: 0
                contentWidth: availableWidth
                clip: true
                Loader {
                    x: 28
                    width: scroll.availableWidth - 56
                    sourceComponent: [general, browsing, sidebar, previews, appearance, about][dialog.page]
                }
            }
        }
    }

    component PageBody: ColumnLayout {
        spacing: 22
        Item { Layout.preferredHeight: 0 }
    }

    Component {
        id: general
        PageBody {
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Start folder")
                    description: AppSettings.startFolder.length > 0 ? AppSettings.startFolder
                                 : qsTr("Home. New windows open here when not given a folder.")
                    Button {
                        visible: dialog.currentFolder.length > 0 && dialog.currentFolder !== AppSettings.startFolder
                        text: qsTr("Use This Folder")
                        onClicked: AppSettings.startFolder = dialog.currentFolder
                    }
                    Button {
                        visible: AppSettings.startFolder.length > 0
                        text: qsTr("Home")
                        onClicked: AppSettings.startFolder = ""
                    }
                }
                SettingRow {
                    title: qsTr("Restore open tabs")
                    description: qsTr("Reopen your tabs from the last session when more than one was open.")
                    ToggleSwitch { checked: AppSettings.restoreTabs; onToggled: AppSettings.restoreTabs = checked }
                }
                SettingRow {
                    title: qsTr("Modified date format")
                    description: qsTr("How modified times appear in lists and details.")
                    Segmented {
                        options: [
                            { label: AppSettings.dateExample("relative"), value: "relative" },
                            { label: AppSettings.dateExample("iso"), value: "iso" },
                            { label: qsTr("Long"), value: "long" }
                        ]
                        value: AppSettings.dateFormat
                        onPicked: (format) => AppSettings.dateFormat = format
                    }
                }
                SettingRow {
                    title: qsTr("Minimize and maximize buttons")
                    description: qsTr("Show them in the header beside Close.")
                    ToggleSwitch { checked: AppSettings.windowButtons; onToggled: AppSettings.windowButtons = checked }
                }
            }
        }
    }

    Component {
        id: browsing
        PageBody {
            SettingsGroup {
                title: qsTr("Opening items")
                Repeater {
                    model: dialog.viewOptions
                    SettingRow {
                        id: clicksRow
                        required property int index
                        required property var modelData
                        divided: index > 0
                        title: qsTr("%1 view").arg(modelData.label)
                        description: qsTr("Files and folders open with a single or a double click.")
                        Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Files"); color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.small }
                        Segmented {
                            options: dialog.clickOptions
                            value: AppSettings.clickCounts[clicksRow.modelData.value + "/files"]
                            onPicked: (count) => AppSettings.setClicks(clicksRow.modelData.value, true, count)
                        }
                        Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Folders"); color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.small }
                        Segmented {
                            options: dialog.clickOptions
                            value: AppSettings.clickCounts[clicksRow.modelData.value + "/folders"]
                            onPicked: (count) => AppSettings.setClicks(clicksRow.modelData.value, false, count)
                        }
                    }
                }
            }
            SettingsGroup {
                title: qsTr("Views and sorting")
                SettingRow {
                    divided: false
                    title: qsTr("Remember view and sort per folder")
                    description: qsTr("Each folder keeps the view and sort you choose there. Other folders use the defaults.")
                    ToggleSwitch { checked: AppSettings.rememberFolderViews; onToggled: AppSettings.rememberFolderViews = checked }
                }
                SettingRow {
                    title: qsTr("Default view")
                    description: qsTr("New tabs open in this view. Folders without their own keep the view you are in.")
                    Segmented { options: dialog.viewOptions; value: AppSettings.defaultView; onPicked: (view) => AppSettings.defaultView = view }
                }
                SettingRow {
                    title: qsTr("Default sort")
                    description: qsTr("Folders without their own sort use this order.")
                    Segmented {
                        options: [
                            { label: qsTr("Name"), value: "name" }, { label: qsTr("Size"), value: "size" },
                            { label: qsTr("Modified"), value: "modified" }, { label: qsTr("Type"), value: "type" }
                        ]
                        value: AppSettings.defaultSort
                        onPicked: (key) => AppSettings.defaultSort = key
                    }
                }
                SettingRow {
                    title: qsTr("Default sort direction")
                    Segmented {
                        options: [{ label: qsTr("Ascending"), value: false }, { label: qsTr("Descending"), value: true }]
                        value: AppSettings.defaultDescending
                        onPicked: (descending) => AppSettings.defaultDescending = descending
                    }
                }
                SettingRow {
                    title: qsTr("Forget all folder settings")
                    description: qsTr("Return every folder to the default view and sort.")
                    Button {
                        text: qsTr("Forget")
                        enabled: AppSettings.hasFolderViews
                        onClicked: AppSettings.forgetFolderViews()
                    }
                }
            }
        }
    }

    Component {
        id: sidebar
        PageBody {
            id: sidebarPage
            property var defaults: dialog.places.defaultPlaces()
            Connections {
                target: dialog.places
                function onHiddenChanged() { sidebarPage.defaults = dialog.places.defaultPlaces() }
            }
            SettingsGroup {
                title: qsTr("Places shown")
                Repeater {
                    model: sidebarPage.defaults
                    SettingRow {
                        required property int index
                        required property var modelData
                        divided: index > 0
                        title: modelData.name
                        ToggleSwitch {
                            Accessible.name: qsTr("Show %1").arg(modelData.name)
                            checked: !modelData.hidden
                            onToggled: dialog.places.setHidden(modelData.key, !checked)
                        }
                    }
                }
            }
            SettingsGroup {
                title: qsTr("Colours")
                Repeater {
                    model: [{ section: 1, name: qsTr("Folders") }, { section: 2, name: qsTr("Pinned") }, { section: 3, name: qsTr("Storage") }]
                    SettingRow {
                        required property int index
                        required property var modelData
                        divided: index > 0
                        title: modelData.name
                        SwatchPicker {
                            value: dialog.places.sectionColors[String(modelData.section)] || ""
                            themeColor: Theme.sectionColor(modelData.section, {})
                            onPicked: (swatch) => dialog.places.setSectionColor(modelData.section, swatch)
                        }
                    }
                }
                SettingRow {
                    title: qsTr("Coloured sidebar icons")
                    description: qsTr("Show each icon in its section's colour. Off, only the current place's icon is coloured.")
                    ToggleSwitch { checked: dialog.places.coloredIcons; onToggled: dialog.places.coloredIcons = checked }
                }
                SettingRow {
                    title: qsTr("Folder order")
                    description: qsTr("Put the folders back in their first order.")
                    Button { text: qsTr("Reset"); onClicked: dialog.places.resetFolderOrder() }
                }
            }
        }
    }

    Component {
        id: previews
        PageBody {
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Preview style")
                    description: qsTr("Open previews in a popup over the window or a panel beside the list.")
                    Segmented {
                        options: [{ label: qsTr("Popup"), value: "popup" }, { label: qsTr("Panel"), value: "panel" }]
                        value: AppSettings.previewStyle
                        onPicked: (style) => AppSettings.previewStyle = style
                    }
                }
                SettingRow {
                    title: qsTr("Show thumbnails")
                    description: qsTr("Show pictures of images instead of file-type icons. Previews stay available.")
                    ToggleSwitch { checked: AppSettings.showThumbnails; onToggled: AppSettings.showThumbnails = checked }
                }
            }
        }
    }

    Component {
        id: appearance
        PageBody {
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Theme")
                    description: qsTr("%1. Files follows the Nyx theme, its text size and density.").arg(dialog.theme.themeName)
                    Button {
                        visible: dialog.theme.canOpenThemeSettings
                        text: qsTr("Open Nyx Settings")
                        onClicked: dialog.theme.openThemeSettings()
                    }
                }
                SettingRow {
                    title: qsTr("Accent")
                    description: qsTr("The colour of selections and highlights in Files.")
                    SwatchPicker {
                        value: dialog.theme.accentChoice
                        themeColor: dialog.theme.colors["accent.primary"]
                        onPicked: (swatch) => dialog.theme.accentChoice = swatch
                    }
                }
                SettingRow {
                    title: qsTr("Text size and density")
                    description: qsTr("%1%, %2. Set for every Nyx app in Nyx Settings.")
                                 .arg(Math.round(dialog.theme.textScale * 100))
                                 .arg(dialog.theme.density === "compact" ? qsTr("compact")
                                      : dialog.theme.density === "relaxed" ? qsTr("relaxed") : qsTr("comfortable"))
                }
            }
        }
    }

    Component {
        id: about
        PageBody {
            id: aboutPage
            readonly property var info: AppSettings.about()
            SettingsGroup {
                SettingRow { divided: false; title: qsTr("Files"); description: qsTr("Version %1 (%2)").arg(aboutPage.info.version).arg(aboutPage.info.commit) }
                SettingRow { title: qsTr("Qt"); description: aboutPage.info.qt }
                SettingRow { title: qsTr("Previews"); description: aboutPage.info.sandbox }
                SettingRow { title: qsTr("Settings file"); description: aboutPage.info.settingsFile }
                SettingRow {
                    title: qsTr("Version info")
                    description: qsTr("For bug reports.")
                    Button { text: qsTr("Copy"); onClicked: AppSettings.copyAbout() }
                }
            }
        }
    }
}
