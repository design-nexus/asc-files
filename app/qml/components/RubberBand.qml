// SPDX-License-Identifier: MIT
import QtQuick

import AscFiles

// Selection by dragging a box from empty space in a view. A press on an entry goes to the entry;
// a press anywhere else starts the box, and the view is asked which rows the box covers as it
// grows. Ctrl adds to what was selected.
MouseArea {
    id: band

    /// The view's flickable, for its scroll offset.
    required property Flickable view
    /// The row at a point in the view's content, or -1.
    property var rowAt: function(x, y) { return -1 }
    /// The rows a rectangle in the view's content covers.
    property var rowsIn: function(rect) { return [] }

    signal selected(var rows, bool add)
    signal cleared()

    property point origin
    property bool adding: false
    readonly property bool active: box.visible

    acceptedButtons: Qt.LeftButton
    onPressed: (mouse) => {
        if (rowAt(mouse.x + view.contentX, mouse.y + view.contentY) >= 0) {
            mouse.accepted = false
            return
        }
        origin = Qt.point(mouse.x + view.contentX, mouse.y + view.contentY)
        adding = (mouse.modifiers & Qt.ControlModifier) !== 0
        box.visible = false
    }
    onPositionChanged: (mouse) => {
        const x = mouse.x + view.contentX, y = mouse.y + view.contentY
        const rect = Qt.rect(Math.min(origin.x, x), Math.min(origin.y, y), Math.abs(x - origin.x), Math.abs(y - origin.y))
        if (!box.visible && rect.width < 4 && rect.height < 4)
            return
        box.visible = true
        box.x = rect.x - view.contentX
        box.y = rect.y - view.contentY
        box.width = rect.width
        box.height = rect.height
        band.selected(rowsIn(rect), adding)
    }
    onReleased: {
        if (!box.visible && !adding)
            band.cleared()
        box.visible = false
    }

    Rectangle {
        id: box
        visible: false
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.15)
        border.width: 1
        border.color: Theme.accent
        radius: 4
    }
}
