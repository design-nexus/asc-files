// SPDX-License-Identifier: MIT
pragma Singleton

import QtQuick

// The current Files look (Nyx Dark), as tokens: the approved baseline for Files 2.0. Values come
// from the running app and its theme (nyx-theme's nyx-dark palette and the mixes it derives).
// Components take every colour, size and font from here, so a later theme changes this file only.
QtObject {
    // Surfaces
    readonly property color window: "#0a0b0f"        // window and sidebar
    readonly property color panel: "#0d0e12"         // the content panel (Nyx Dark background)
    readonly property color field: "#14151b"         // search field, segmented controls, buttons
    readonly property color raised: "#191a21"        // the active tab
    readonly property color control: "#1d1e26"       // a checked segment, key hints
    readonly property color panelEdge: Qt.rgba(231 / 255, 232 / 255, 238 / 255, 0.07)
    readonly property color rule: Qt.rgba(231 / 255, 232 / 255, 238 / 255, 0.06)

    // Text
    readonly property color text: "#e7e8ee"
    readonly property color textSoft: "#d4d6df"     // sidebar places
    readonly property color textDim: "#a2a5ba"      // secondary cells, inactive icons
    readonly property color textMute: "#8a8ea8"     // headers, counts
    readonly property color textDisabled: "#4a4d5c"

    // Accent and states
    readonly property color accent: "#3d8ae8"
    readonly property color accentText: "#6aa6ef"
    readonly property color selection: "#15223a"    // mix(background, accent, 0.18)
    readonly property color selectionEdge: Qt.rgba(61 / 255, 138 / 255, 232 / 255, 0.55)
    readonly property color hover: "#10151e"        // mix(background, accent, 0.07)
    readonly property color danger: "#f2655f"
    readonly property color warning: "#eeb43f"
    readonly property color success: "#59d45b"

    // Sidebar sections: a heading and its icons share the section's colour.
    readonly property color sectionFolders: "#3d8ae8"
    readonly property color sectionPinned: "#e8a33d"
    readonly property color sectionStorage: "#8b6cf0"

    // Swatches a sidebar section can take, by the names the settings keep (they follow the theme).
    readonly property var swatches: ({
        "blue": "#3d8ae8", "cyan": "#3cc4c9", "green": "#59d45b", "yellow": "#eeb43f",
        "orange": "#e8a33d", "red": "#f2655f", "pink": "#c95fc6", "purple": "#8b6cf0"
    })
    readonly property var swatchNames: ["blue", "cyan", "green", "yellow", "orange", "red", "pink", "purple"]

    // A section's colour: its chosen swatch, else the theme's ("1" Folders, "2" Pinned, "3" Storage).
    function sectionColor(section, chosen) {
        const swatch = chosen ? chosen[String(section)] : undefined
        if (swatch !== undefined && swatches[swatch] !== undefined)
            return swatches[swatch]
        return section === 2 ? sectionPinned : section === 3 ? sectionStorage : sectionFolders
    }

    readonly property int railWidth: 56

    // Type: Inter at 13.5 px, the system's sans.
    readonly property string family: "Inter"
    readonly property real body: 10.125            // points (13.5 px at 96 dpi)
    readonly property real small: 9                // 12 px
    readonly property real tiny: 8.625             // 11.5 px, headings and column titles
    readonly property real title: 10.875           // 14.5 px, the app name

    // Shape and rhythm
    readonly property int radius: 8
    readonly property int panelRadius: 14
    readonly property int headerHeight: 48
    readonly property int sidebarWidth: 229
    readonly property int placeHeight: 33
    readonly property int rowHeight: 30
    readonly property int controlSize: 32
    readonly property int iconSize: 16
}
