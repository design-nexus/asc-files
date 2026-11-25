// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// Open With, as a submenu: the default program first, then others that open such files, then
// every program in a dialog.
ContextMenu {
    id: menu

    required property var apps
    property var paths: []
    property var choices: []
    /// "Other Application…" was chosen.
    signal othersRequested(var paths)
    signal failed(string message)

    function load(forPaths) {
        paths = forPaths
        choices = forPaths.length > 0 ? apps.appsFor(forPaths, 6) : []
    }

    title: qsTr("Open With")

    Instantiator {
        model: menu.choices
        delegate: MenuItem {
            required property var modelData
            text: modelData.isDefault ? qsTr("%1 (default)").arg(modelData.name) : modelData.name
            onTriggered: {
                const why = menu.apps.launch(modelData.id, menu.paths)
                if (why.length > 0) menu.failed(why)
            }
        }
        onObjectAdded: (index, item) => menu.insertItem(index, item)
        onObjectRemoved: (index, item) => menu.removeItem(item)
    }
    MenuSeparator { visible: menu.choices.length > 0; height: visible ? implicitHeight : 0 }
    MenuItem { text: qsTr("Other Application…"); onTriggered: menu.othersRequested(menu.paths) }
}
