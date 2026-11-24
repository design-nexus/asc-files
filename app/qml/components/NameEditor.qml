// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic

import AscFiles

// The field an item's name is edited in, in place. It opens with the name before the extension
// selected (all of a folder's); Enter keeps the new name, Escape or leaving it keeps the old.
TextField {
    id: field

    property string original
    property bool isFolder: false
    property bool done: false

    signal committed(string name)
    signal cancelled()

    function begin() {
        text = original
        done = false
        forceActiveFocus()
        const dot = isFolder ? -1 : original.lastIndexOf(".")
        select(0, dot > 0 ? dot : original.length)
    }

    color: Theme.text
    selectionColor: Theme.accent
    selectedTextColor: Theme.text
    font.family: Theme.family
    font.pointSize: Theme.body
    leftPadding: 6
    rightPadding: 6
    topPadding: 2
    bottomPadding: 2
    background: Rectangle { radius: 6; color: Theme.field; border.width: 1; border.color: Theme.accent }

    Keys.onReturnPressed: { done = true; field.committed(text.trim()) }
    Keys.onEnterPressed: { done = true; field.committed(text.trim()) }
    Keys.onEscapePressed: { done = true; field.cancelled() }
    onActiveFocusChanged: if (!activeFocus && !done) { done = true; field.cancelled() }
}
