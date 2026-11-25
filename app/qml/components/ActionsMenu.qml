// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// The user's actions on offer for what was right-clicked, under Actions (those placed at the
// menu's top level are put there by the window). One that can't run (a missing interpreter or
// program) shows, greyed.
ContextMenu {
    id: menu

    /// Entries as the actions model offers them.
    property var actions: []
    signal chosen(var action)

    title: qsTr("Actions")

    Instantiator {
        model: menu.actions
        delegate: MenuItem {
            required property var modelData
            text: modelData.name
            enabled: modelData.available
            onTriggered: menu.chosen(modelData)
        }
        onObjectAdded: (index, item) => menu.insertItem(index, item)
        onObjectRemoved: (index, item) => menu.removeItem(item)
    }
}
