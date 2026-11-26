// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Layouts

import AscFiles

// A short message at the foot of the window, for something refused or done; it goes after a
// while, or when clicked.
Rectangle {
    id: notice

    // Read out as it shows.
    Accessible.role: Accessible.AlertMessage
    Accessible.name: label.text

    function show(message) {
        label.text = message
        opacity = 1
        hide.restart()
    }

    visible: opacity > 0
    opacity: 0
    width: Math.min(560, label.implicitWidth + 40)
    height: label.implicitHeight + 20
    radius: Theme.radius + 2
    color: Theme.raised
    border.width: 1
    border.color: Theme.panelEdge
    Behavior on opacity { NumberAnimation { duration: 150 } }

    Text {
        id: label
        anchors.fill: parent
        anchors.margins: 10
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        wrapMode: Text.Wrap
        verticalAlignment: Text.AlignVCenter
        color: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.body
    }
    TapHandler { onTapped: notice.opacity = 0 }
    Timer { id: hide; interval: 5000; onTriggered: notice.opacity = 0 }
}
