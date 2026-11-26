// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// Makes or changes a custom action, as legacy's editor did: General (name, id, description,
// enabled), Script (Python, Bash or a command; the recipe library for scripts) and Behavior
// (what it is offered for, how it runs). Saved whole, checked by the backend as action.toml
// is; what is wrong is said, and the draft stays.
Popup {
    id: editor
    objectName: "actionEditor"

    required property var actions
    /// Editing an existing action (its id stays); else making one.
    property bool existing: false
    property int tab: 0
    property string error
    // The draft.
    property string name
    property string actionId
    property bool idEdited: false
    property string description
    property string icon
    property bool actionEnabled: true
    property string menu: "submenu"
    property string runtime: "python"
    property string entrypoint: "main.py"
    property string program
    property string args
    property string mode: "whole-selection"
    property string errorPolicy: "continue"
    property string workingDirectory: "parent"
    property bool confirm: false
    property bool files: true
    property bool folders: true
    property string extensions
    property var mimeTypes: []
    property int minItems: 1
    property string maxItems
    /// Each runtime's script, kept while switching between them.
    property var scripts: ({ python: "", bash: "" })
    property var examples: []

    // As the engine's `suggest_id`: lowercase letters and digits, dashes between.
    function suggestId(text) {
        let slug = ""
        for (const character of text) {
            if (/[A-Za-z0-9]/.test(character)) slug += character.toLowerCase()
            else if (slug.length > 0 && !slug.endsWith("-")) slug += "-"
        }
        slug = slug.replace(/^-+|-+$/g, "")
        return (slug.length > 0 ? slug : "action").substring(0, 64)
    }
    // The text areas show the draft when it changes under them (they report what is typed, and
    // aren't bound to it, so the cursor stays where it is).
    function showDraft() {
        scriptArea.text = runtime === "command" ? "" : (scripts[runtime] || "")
        argsArea.text = args
    }
    function reset() {
        existing = false; tab = 0; error = ""
        name = ""; actionId = ""; idEdited = false; description = ""; icon = ""; actionEnabled = true; menu = "submenu"
        runtime = "python"; entrypoint = "main.py"; program = ""; args = ""
        mode = "whole-selection"; errorPolicy = "continue"; workingDirectory = "parent"; confirm = false
        files = true; folders = true; extensions = ""; mimeTypes = []; minItems = 1; maxItems = ""
        scripts = { python: "", bash: "" }
        showDraft()
    }
    function create() {
        reset()
        actions.readExamples()
        open()
    }
    function edit(id) {
        reset()
        existing = true
        actions.readExamples()
        actions.read(id)
    }
    function load(action) {
        const d = action.definition
        const when = d.when || {}
        const run = d.run || {}
        name = d.name; actionId = d.id; idEdited = true; description = d.description || ""; icon = d.icon || ""
        actionEnabled = d.enabled !== false; menu = d.menu || "submenu"
        runtime = run.runtime || "python"; entrypoint = run.entrypoint || (runtime === "bash" ? "run.sh" : "main.py")
        program = run.program || ""; args = (run.args || []).join("\n")
        mode = run.mode || "whole-selection"; errorPolicy = run.on_error || "continue"
        workingDirectory = run.working_directory || "parent"; confirm = run.confirm === true
        const kinds = when.kinds || []
        files = kinds.length === 0 || kinds.indexOf("file") >= 0
        folders = kinds.length === 0 || kinds.indexOf("folder") >= 0
        extensions = (when.extensions || []).join(", ")
        mimeTypes = when.mime_types || []
        minItems = when.min_items || 1
        maxItems = when.max_items !== undefined && when.max_items !== null ? String(when.max_items) : ""
        const copy = { python: scripts.python, bash: scripts.bash }
        if (action.script && runtime !== "command") copy[runtime] = action.script.contents
        scripts = copy
        showDraft()
        open()
    }
    function useExample(example) {
        const copy = { python: scripts.python, bash: scripts.bash }
        copy[example.runtime] = example.script
        scripts = copy
        runtime = example.runtime
        entrypoint = example.runtime === "bash" ? "run.sh" : "main.py"
        mode = example.mode
        files = true
        folders = example.folders
        extensions = example.extensions.join(", ")
        if (name.length === 0) name = example.name
        if (description.length === 0) description = example.description
        showDraft()
    }
    function definition() {
        const kinds = []
        if (files) kinds.push("file")
        if (folders) kinds.push("folder")
        const when = { kinds: kinds.length === 2 ? [] : kinds, min_items: minItems }
        const listed = extensions.split(/[\s,]+/).map(e => e.replace(/^\./, "").toLowerCase()).filter(e => e.length > 0)
        if (listed.length > 0) when.extensions = listed
        if (mimeTypes.length > 0) when.mime_types = mimeTypes
        if (maxItems.trim().length > 0) when.max_items = parseInt(maxItems)
        const run = { runtime: runtime, mode: mode, on_error: errorPolicy, working_directory: workingDirectory, confirm: confirm }
        if (runtime === "command") {
            run.program = program.trim()
            const lines = args.split("\n").filter(line => line.length > 0)
            if (lines.length > 0) run.args = lines
        } else {
            run.entrypoint = entrypoint.trim()
        }
        const d = { schema_version: 1, id: actionId, name: name.trim(), enabled: actionEnabled, menu: menu, when: when, run: run }
        if (description.trim().length > 0) d.description = description.trim()
        if (icon.trim().length > 0) d.icon = icon.trim()
        return d
    }
    // What can be said before the backend checks the rest; the tab to show it on.
    function problem() {
        if (name.trim().length === 0) return [0, qsTr("Enter a name")]
        if (runtime === "command" && program.trim().length === 0) return [1, qsTr("Enter the program to run")]
        if (runtime !== "command" && entrypoint.trim().length === 0) return [1, qsTr("Enter a script file name")]
        if (!files && !folders) return [2, qsTr("Choose Files, Folders, or both")]
        if (maxItems.trim().length > 0 && !/^\d+$/.test(maxItems.trim())) return [2, qsTr("The maximum item count must be a number")]
        return null
    }
    function save() {
        const wrong = problem()
        if (wrong) { tab = wrong[0]; error = wrong[1]; return }
        error = ""
        actions.write(definition(), runtime === "command" ? {} : { fileName: entrypoint.trim(), contents: scripts[runtime] }, !existing)
    }

    Connections {
        target: editor.actions
        enabled: editor.opened || editor.existing
        function onReadDone(action) { if (editor.existing && !editor.opened) editor.load(action) }
        function onSaved(id) { if (editor.opened) editor.close() }
        function onFailed(message) { if (editor.opened) editor.error = message }
        function onExamplesRead(list) { editor.examples = list }
    }
    onNameChanged: if (!idEdited && !existing) actionId = suggestId(name)

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(720, parent ? parent.width - 48 : 720)
    height: Math.min(620, parent ? parent.height - 48 : 620)
    modal: true
    focus: true
    padding: 20
    // Clicking outside keeps the draft; Escape, Cancel or Close drop it.
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    component Field: TextField {
        implicitHeight: 32
        leftPadding: 10
        color: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.body
        placeholderTextColor: Theme.textMute
        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: parent.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
    }
    component Label: Text {
        color: Theme.textDim
        font.family: Theme.family
        font.pointSize: Theme.small
    }

    contentItem: ColumnLayout {
        Accessible.role: Accessible.Dialog
        Accessible.name: editor.existing ? qsTr("Edit “%1”").arg(editor.name) : qsTr("New action")
        spacing: 12
        RowLayout {
            Layout.fillWidth: true
            Text {
                Layout.fillWidth: true
                text: editor.existing ? qsTr("Edit “%1”").arg(editor.name) : qsTr("New action")
                elide: Text.ElideRight
                color: Theme.text
                font.family: Theme.family
                font.pointSize: Theme.title
                font.weight: Font.DemiBold
            }
            Segmented {
                options: [{ label: qsTr("General"), value: 0 }, { label: qsTr("Script"), value: 1 }, { label: qsTr("Behavior"), value: 2 }]
                value: editor.tab
                onPicked: (tab) => editor.tab = tab
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: editor.tab

            // General
            ColumnLayout {
                spacing: 8
                Label { text: qsTr("Name") }
                Field { Layout.fillWidth: true; text: editor.name; placeholderText: qsTr("Resize images"); onTextEdited: editor.name = text }
                Label { text: qsTr("Id: its folder's name; it can't change once saved.") }
                Field {
                    Layout.fillWidth: true
                    enabled: !editor.existing
                    text: editor.actionId
                    onTextEdited: { editor.actionId = text; editor.idEdited = true }
                }
                Label { text: qsTr("Description") }
                Field { Layout.fillWidth: true; text: editor.description; onTextEdited: editor.description = text }
                RowLayout {
                    spacing: 10
                    ToggleSwitch { checked: editor.actionEnabled; onToggled: editor.actionEnabled = checked; Accessible.name: qsTr("Enabled") }
                    Label { text: qsTr("Enabled: offered in the menus") }
                }
                Item { Layout.fillHeight: true }
            }

            // Script
            ColumnLayout {
                spacing: 8
                RowLayout {
                    spacing: 8
                    Segmented {
                        options: [{ label: "Python", value: "python" }, { label: "Bash", value: "bash" }, { label: qsTr("Command"), value: "command" }]
                        value: editor.runtime
                        onPicked: (runtime) => {
                            editor.runtime = runtime
                            if (runtime !== "command") editor.entrypoint = runtime === "bash" ? "run.sh" : "main.py"
                            editor.showDraft()
                        }
                    }
                    Item { Layout.fillWidth: true }
                    Button {
                        visible: editor.runtime !== "command" && editor.examples.length > 0
                        text: qsTr("Library")
                        menu: true
                        onClicked: library.popup(this, 0, height + 4)
                    }
                }
                ContextMenu {
                    id: library
                    Instantiator {
                        model: editor.examples
                        delegate: MenuItem {
                            required property var modelData
                            text: modelData.name + " · " + modelData.requirements
                            onTriggered: editor.useExample(modelData)
                        }
                        onObjectAdded: (index, item) => library.insertItem(index, item)
                        onObjectRemoved: (index, item) => library.removeItem(item)
                    }
                }
                RowLayout {
                    visible: editor.runtime !== "command"
                    spacing: 8
                    Label { text: qsTr("File") }
                    Field { Layout.preferredWidth: 200; text: editor.entrypoint; onTextEdited: editor.entrypoint = text }
                }
                ScrollView {
                    visible: editor.runtime !== "command"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    TextArea {
                        id: scriptArea
                        objectName: "scriptText"
                        placeholderText: editor.runtime === "bash" ? "#!/usr/bin/env bash\n" : "#!/usr/bin/env python3\nfrom nyx_files_actions import context\n"
                        placeholderTextColor: Theme.textMute
                        onTextChanged: if (activeFocus) { const copy = { python: editor.scripts.python, bash: editor.scripts.bash }; copy[editor.runtime] = text; editor.scripts = copy }
                        wrapMode: TextEdit.NoWrap
                        color: Theme.text
                        font.family: Theme.monoFamily
                        font.pointSize: Theme.small
                        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: Theme.panelEdge }
                    }
                }
                Label { visible: editor.runtime === "command"; text: qsTr("Program") }
                Field { visible: editor.runtime === "command"; Layout.fillWidth: true; text: editor.program; placeholderText: "magick"; onTextEdited: editor.program = text }
                Label {
                    visible: editor.runtime === "command"
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: qsTr("Arguments, one per line. {path} is the item (per item), {paths} every item (whole selection), {parent} the folder; nothing goes through a shell.")
                }
                ScrollView {
                    visible: editor.runtime === "command"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    TextArea {
                        id: argsArea
                        onTextChanged: if (activeFocus) editor.args = text
                        color: Theme.text
                        font.family: Theme.monoFamily
                        font.pointSize: Theme.small
                        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: Theme.panelEdge }
                    }
                }
            }

            // Behavior
            ColumnLayout {
                spacing: 10
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("Runs") }
                    Segmented {
                        options: [{ label: qsTr("Once for all"), value: "whole-selection" }, { label: qsTr("Once per item"), value: "per-item" }]
                        value: editor.mode
                        onPicked: (mode) => editor.mode = mode
                    }
                }
                RowLayout {
                    visible: editor.mode === "per-item"
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("When one fails") }
                    Segmented {
                        options: [{ label: qsTr("Go on"), value: "continue" }, { label: qsTr("Stop"), value: "stop" }]
                        value: editor.errorPolicy
                        onPicked: (policy) => editor.errorPolicy = policy
                    }
                }
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("Working folder") }
                    Segmented {
                        options: [{ label: qsTr("The items' folder"), value: "parent" }, { label: qsTr("Home"), value: "home" }, { label: qsTr("The action's"), value: "action" }]
                        value: editor.workingDirectory
                        onPicked: (folder) => editor.workingDirectory = folder
                    }
                }
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("In the menu") }
                    Segmented {
                        options: [{ label: qsTr("Under Actions"), value: "submenu" }, { label: qsTr("At the top level"), value: "top" }]
                        value: editor.menu
                        onPicked: (menu) => editor.menu = menu
                    }
                }
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("Offered for") }
                    Bit { text: qsTr("Files"); checked: editor.files; onToggled: editor.files = checked }
                    Bit { text: qsTr("Folders"); checked: editor.folders; onToggled: editor.folders = checked }
                }
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("Extensions") }
                    Field { Layout.fillWidth: true; text: editor.extensions; placeholderText: qsTr("Any (or: jpg, png)"); onTextEdited: editor.extensions = text }
                }
                RowLayout {
                    spacing: 10
                    Label { Layout.preferredWidth: 150; text: qsTr("Most items") }
                    Field { Layout.preferredWidth: 120; text: editor.maxItems; placeholderText: qsTr("No limit"); onTextEdited: editor.maxItems = text }
                }
                RowLayout {
                    spacing: 10
                    ToggleSwitch { checked: editor.confirm; onToggled: editor.confirm = checked; Accessible.name: qsTr("Ask before running") }
                    Label { text: qsTr("Ask before running") }
                }
                Item { Layout.fillHeight: true }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: text.length > 0
            text: editor.error
            wrapMode: Text.Wrap
            color: Theme.danger
            font.family: Theme.family
            font.pointSize: Theme.small
        }
        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: qsTr("Actions run with your permissions, without a sandbox.")
            }
            Button { text: qsTr("Cancel"); onClicked: editor.close() }
            Button { text: editor.existing ? qsTr("Save Changes") : qsTr("Create Action"); onClicked: editor.save() }
        }
    }

    component Bit: AbstractButton {
        id: bit
        checkable: true
        implicitWidth: bitLabel.implicitWidth + 20
        implicitHeight: 28
        Accessible.role: Accessible.CheckBox
        background: Rectangle {
            radius: Theme.radius - 2
            color: bit.checked ? Theme.selection : Theme.field
            border.width: 1
            border.color: bit.checked ? Theme.selectionEdge : Theme.panelEdge
        }
        contentItem: Text {
            id: bitLabel
            text: bit.text
            color: bit.checked ? Theme.text : Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
    }
}
