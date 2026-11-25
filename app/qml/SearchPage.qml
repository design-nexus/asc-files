// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A search's results: where it searched (Home and the drives, or the folder it began in), how it
// is going, and what it found, best first, each with the folder it is in.
ColumnLayout {
    id: page

    required property var browser
    required property var search
    required property var places
    /// The folder the search began in, offered as a narrower place to search.
    property string origin
    readonly property alias currentPath: results.currentPath
    readonly property string status: search.running ? qsTr("Searching…")
        : search.query.length === 0 ? ""
        : search.truncated ? qsTr("%n results; it stopped at its limit", "", search.count)
        : qsTr("%n results", "", search.count)
    signal fileActivated(string path)
    signal contextRequested(int row)

    function focusList() { results.focusList() }
    function step(delta) { results.step(delta) }
    function searchIn(roots) { search.search(search.query, roots); search.searchNow() }

    spacing: 0

    RowLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 16
        Layout.rightMargin: 16
        Layout.topMargin: 6
        spacing: 6
        Text { text: qsTr("Search in"); color: Theme.textMute; font.family: Theme.family; font.pointSize: Theme.small }
        TabChip {
            closable: false
            text: qsTr("Home and drives")
            current: page.search.roots.length !== 1 || page.search.roots[0] !== page.origin
            onClicked: page.searchIn(page.places.searchRoots())
        }
        TabChip {
            closable: false
            visible: page.origin.length > 0
            text: page.origin.split("/").pop() || "/"
            current: page.search.roots.length === 1 && page.search.roots[0] === page.origin
            onClicked: page.searchIn([page.origin])
        }
        Item { Layout.fillWidth: true }
        BusyIndicator { running: page.search.running; visible: running; implicitWidth: 20; implicitHeight: 20 }
    }
    ItemsView {
        id: results
        Layout.fillWidth: true
        Layout.fillHeight: true
        browser: page.browser
        items: page.search
        emptyText: page.search.query.length === 0 ? qsTr("Type to search")
                 : page.search.running ? "" : qsTr("Nothing matches “%1”").arg(page.search.query)
        onFileActivated: (path) => page.fileActivated(path)
        onContextRequested: (row) => page.contextRequested(row)
    }
}
