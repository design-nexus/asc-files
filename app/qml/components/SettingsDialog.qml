// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Dialogs
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
    /// The window's search, which checks exclusion rules with the backend.
    required property var search
    /// The user's custom actions.
    required property var actions
    /// Files as the desktop's file chooser and file manager (none in tests).
    property var integrations: null
    /// The folder the window shows, offered as the start folder.
    property string currentFolder
    property int page: 0

    readonly property var pages: [
        { title: qsTr("General"), detail: qsTr("Start folder, tabs, dates") },
        { title: qsTr("Browsing"), detail: qsTr("Opening items, views, sorting") },
        { title: qsTr("Sidebar"), detail: qsTr("Places shown, colours") },
        { title: qsTr("Search"), detail: qsTr("Typing to filter, limits, exclusions") },
        { title: qsTr("Transfers"), detail: qsTr("Dropping files") },
        { title: qsTr("Actions"), detail: qsTr("Your own commands for files") },
        { title: qsTr("Previews"), detail: qsTr("Preview style, thumbnails") },
        { title: qsTr("Appearance"), detail: qsTr("Theme, accent, text size") },
        { title: qsTr("Integrations"), detail: qsTr("File chooser, file manager") },
        { title: qsTr("About"), detail: qsTr("Version, diagnostics") }
    ]
    readonly property var viewOptions: [
        { label: qsTr("List"), value: 0 }, { label: qsTr("Icons"), value: 1 }, { label: qsTr("Columns"), value: 2 },
        { label: qsTr("Compact"), value: 3 }, { label: qsTr("Gallery"), value: 4 }, { label: qsTr("Dual Pane"), value: 5 }
    ]
    // Dual Pane's panes open items as List does.
    readonly property var clickViews: viewOptions.slice(0, 5)
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
        Accessible.role: Accessible.Dialog
        Accessible.name: qsTr("Settings")
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
                    sourceComponent: [general, browsing, sidebar, searching, transfers, actionsPage, previews, appearance, integrationsPage, about][dialog.page]
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
                    model: dialog.clickViews
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
                title: qsTr("Keyboard")
                SettingRow {
                    divided: false
                    title: AppSettings.tenxer ? qsTr("10xer mode (experimental feature, under active development)") : qsTr("10xer mode")
                    description: qsTr("Opinionated keyboard-centric mode with Yazi-style navigation. Disables some features. Toggle with Ctrl+Shift+M.")
                    ToggleSwitch { checked: AppSettings.tenxer; onToggled: AppSettings.tenxer = checked }
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
        id: searching
        PageBody {
            id: searchPage
            property string refused
            Connections {
                target: dialog.search
                function onExclusionRefused(why) { searchPage.refused = why }
            }
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Type to filter")
                    description: AppSettings.tenxer ? qsTr("Not used in 10xer mode.") : qsTr("Typing in a folder filters it to the names that match.")
                    ToggleSwitch { checked: AppSettings.typeToFilter; onToggled: AppSettings.typeToFilter = checked }
                }
                SettingRow {
                    title: qsTr("Search time limit")
                    description: qsTr("A search stops after this long. Slow or network drives need longer to look through every folder.")
                    Segmented {
                        options: [
                            { label: qsTr("10 s"), value: 10 }, { label: qsTr("30 s"), value: 30 }, { label: qsTr("1 min"), value: 60 },
                            { label: qsTr("5 min"), value: 300 }, { label: qsTr("15 min"), value: 900 }, { label: qsTr("No limit"), value: 0 }
                        ]
                        value: AppSettings.searchSeconds
                        onPicked: (seconds) => AppSettings.searchSeconds = seconds
                    }
                }
                SettingRow {
                    title: qsTr("Search entry limit")
                    description: qsTr("A search stops after looking at this many files and folders. Higher limits use more memory.")
                    Segmented {
                        options: [
                            { label: Number(200000).toLocaleString(Qt.locale(), "f", 0), value: 200000 },
                            { label: Number(500000).toLocaleString(Qt.locale(), "f", 0), value: 500000 },
                            { label: Number(1000000).toLocaleString(Qt.locale(), "f", 0), value: 1000000 },
                            { label: Number(5000000).toLocaleString(Qt.locale(), "f", 0), value: 5000000 },
                            { label: qsTr("No limit"), value: 0 }
                        ]
                        value: AppSettings.searchEntries
                        onPicked: (entries) => AppSettings.searchEntries = entries
                    }
                }
            }
            SettingsGroup {
                title: qsTr("Left out of searches")
                Repeater {
                    model: AppSettings.searchExclusions
                    SettingRow {
                        required property int index
                        required property string modelData
                        divided: index > 0
                        title: modelData
                        description: modelData.startsWith("/") ? qsTr("This folder and all in it") : qsTr("Every folder with this name, and all in them")
                        IconButton {
                            label: qsTr("Remove %1").arg(modelData)
                            iconPath: "m4 4 8 8M12 4l-8 8"
                            onClicked: AppSettings.removeSearchExclusion(modelData)
                        }
                    }
                }
                SettingRow {
                    divided: AppSettings.searchExclusions.length > 0
                    title: qsTr("Leave out")
                    description: searchPage.refused.length > 0 ? searchPage.refused
                                 : qsTr("A folder name, such as cache, or a folder's path, such as ~/Private.")
                    TextField {
                        id: rule
                        objectName: "exclusionField"
                        width: 200
                        height: 30
                        color: Theme.text
                        font.family: Theme.family
                        font.pointSize: Theme.body
                        placeholderText: qsTr("Folder name or path")
                        placeholderTextColor: Theme.textMute
                        leftPadding: 10
                        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: rule.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
                        onTextEdited: searchPage.refused = ""
                        onAccepted: add.clicked()
                    }
                    Button {
                        id: add
                        text: qsTr("Add")
                        enabled: rule.text.trim().length > 0
                        onClicked: { dialog.search.addExclusion(rule.text); rule.text = "" }
                    }
                }
            }
        }
    }

    Component {
        id: transfers
        PageBody {
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Dropping onto another drive")
                    description: qsTr("On the same drive, a drop moves. Onto another drive or a share, it does this.")
                    Segmented {
                        options: [{ label: qsTr("Copy"), value: "copy" }, { label: qsTr("Move"), value: "move" }, { label: qsTr("Ask"), value: "ask" }]
                        value: AppSettings.crossDriveDrop
                        onPicked: (choice) => AppSettings.crossDriveDrop = choice
                    }
                }
                SettingRow {
                    title: qsTr("Open the folder after dropping")
                    description: qsTr("Show the folder items were dropped into.")
                    ToggleSwitch { checked: AppSettings.openAfterDrop; onToggled: AppSettings.openAfterDrop = checked }
                }
            }
        }
    }

    ActionEditor { id: actionEditor; actions: dialog.actions }
    // Asks for a folder, for importing an action or exporting one; made when wanted.
    Loader {
        id: folderAsk
        active: false
        property var chosen: null
        property string title
        function ask(title, chosen) {
            folderAsk.title = title
            folderAsk.chosen = chosen
            active = true
            item.open()
        }
        sourceComponent: FolderDialog {
            title: folderAsk.title
            onAccepted: folderAsk.chosen(decodeURIComponent(selectedFolder.toString().replace(/^file:\/\//, "")))
        }
    }

    Component {
        id: actionsPage
        PageBody {
            Text {
                Layout.fillWidth: true
                text: qsTr("Your own scripts and programs, on the item menus. They run with your permissions, without a sandbox; one you import is off until you turn it on.")
                wrapMode: Text.Wrap
                color: Theme.textDim
                font.family: Theme.family
                font.pointSize: Theme.small
            }
            SettingsGroup {
                title: qsTr("Your actions")
                Text {
                    visible: dialog.actions.actions.length === 0
                    Layout.margins: 16
                    text: qsTr("No actions yet.")
                    color: Theme.textMute
                    font.family: Theme.family
                    font.pointSize: Theme.body
                }
                Repeater {
                    model: dialog.actions.actions
                    SettingRow {
                        id: actionRow
                        required property int index
                        required property var modelData
                        divided: index > 0
                        title: modelData.name
                        description: (modelData.description || (modelData.runtime + " · " + (modelData.mode === "per-item" ? qsTr("once per item") : qsTr("once for all"))))
                                     + (modelData.available ? "" : "\n" + (modelData.reason || qsTr("It can't run here.")))
                        ToggleSwitch {
                            anchors.verticalCenter: parent.verticalCenter
                            Accessible.name: qsTr("Turn on %1").arg(actionRow.modelData.name)
                            checked: actionRow.modelData.enabled
                            onToggled: dialog.actions.setEnabled(actionRow.modelData.id, checked)
                        }
                        IconButton {
                            label: qsTr("Edit %1").arg(actionRow.modelData.name)
                            iconPath: "M3 13h3l7-7-3-3-7 7zM9 4l3 3"
                            onClicked: actionEditor.edit(actionRow.modelData.id)
                        }
                        IconButton {
                            id: more
                            label: qsTr("More")
                            iconPath: "M4 8h.01M8 8h.01M12 8h.01"
                            onClicked: moreMenu.popup(more, 0, more.height)
                        }
                        ContextMenu {
                            id: moreMenu
                            MenuItem { text: qsTr("Duplicate"); onTriggered: dialog.actions.duplicate(actionRow.modelData.id) }
                            MenuItem {
                                text: qsTr("Export…")
                                onTriggered: folderAsk.ask(qsTr("Export this action to a folder"), (folder) => dialog.actions.exportTo(actionRow.modelData.id, folder))
                            }
                            MenuSeparator {}
                            MenuItem {
                                text: qsTr("Delete…")
                                onTriggered: deleteConfirm.ask(qsTr("Delete “%1”?").arg(actionRow.modelData.name),
                                                               qsTr("Its folder and script are deleted."), qsTr("Delete"),
                                                               () => dialog.actions.remove(actionRow.modelData.id))
                            }
                        }
                    }
                }
            }
            SettingsGroup {
                visible: dialog.actions.failures.length > 0
                title: qsTr("Problems")
                Repeater {
                    model: dialog.actions.failures
                    SettingRow {
                        required property int index
                        required property var modelData
                        divided: index > 0
                        title: modelData.directory
                        description: modelData.reason
                    }
                }
            }
            RowLayout {
                spacing: 8
                Button { text: qsTr("New Action…"); onClicked: actionEditor.create() }
                Button {
                    text: qsTr("Import…")
                    onClicked: folderAsk.ask(qsTr("Import an action folder"), (folder) => dialog.actions.importFrom(folder))
                }
            }
            ConfirmDialog { id: deleteConfirm }
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
                    title: qsTr("Autoplay media previews")
                    description: qsTr("Start playing audio and video as soon as their preview opens.")
                    ToggleSwitch { checked: AppSettings.autoplayMedia; onToggled: AppSettings.autoplayMedia = checked }
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
        id: integrationsPage
        PageBody {
            Component.onCompleted: if (dialog.integrations) dialog.integrations.refresh()
            SettingsGroup {
                SettingRow {
                    divided: false
                    title: qsTr("Choose files for other apps")
                    description: qsTr("Open and save files from other apps in Files instead of the desktop's own chooser.")
                    ToggleSwitch {
                        enabled: dialog.integrations !== null && !dialog.integrations.busy
                        checked: dialog.integrations !== null && dialog.integrations.chooser
                        onToggled: dialog.integrations.setChooser(checked)
                    }
                }
                SettingRow {
                    title: qsTr("Default file manager")
                    description: qsTr("Open folders, and “Show in folder” from other apps, in Files. Turning it off gives folders back to the file manager before.")
                    ToggleSwitch {
                        enabled: dialog.integrations !== null && !dialog.integrations.busy
                        checked: dialog.integrations !== null && dialog.integrations.fileManager
                        onToggled: dialog.integrations.setFileManager(checked)
                    }
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
