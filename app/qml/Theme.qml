// SPDX-License-Identifier: MIT
pragma Singleton

import QtQuick

import AscFiles.System

// The look of Files, as tokens, taken from the Nyx system theme (SystemTheme follows it live):
// switching the theme, its accent, the text size or the density in Settings reaches Files at
// once. The roles are those of the current Files look; each maps to a Nyx token, or is mixed
// from them as Nyx mixes its own (a hover is the background tinted with the accent).
QtObject {
    id: theme

    function mix(base, over, amount) {
        return Qt.tint(base, Qt.rgba(over.r, over.g, over.b, amount))
    }
    function token(name) { return SystemTheme.color(name) }
    /// `name` as styled text with each place it holds `found` (any case) in the accent colour,
    /// bold; plain `name` when there is nothing to mark (10xer's find highlights).
    function marked(name, found) {
        const escape = (text) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        if (!found || found.length === 0) return escape(name)
        const lower = name.toLowerCase()
        const query = found.toLowerCase()
        let out = ""
        let at = 0
        for (let hit = lower.indexOf(query); hit >= 0; hit = lower.indexOf(query, at)) {
            out += escape(name.substring(at, hit)) + "<b><font color=\"" + accent + "\">" + escape(name.substring(hit, hit + query.length)) + "</font></b>"
            at = hit + query.length
        }
        return out + escape(name.substring(at))
    }
    readonly property bool light: SystemTheme.light

    // Surfaces
    readonly property color panel: SystemTheme.colors["surface.primary"]        // the content panel
    readonly property color window: mix(panel, token("shadow.color"), light ? 0.04 : 0.3)  // window and sidebar
    readonly property color field: SystemTheme.colors["surface.secondary"]      // search field, segmented controls
    readonly property color raised: SystemTheme.colors["surface.tertiary"]      // the active tab, cards
    readonly property color control: SystemTheme.colors["surface.alternate"]    // a checked segment, key hints
    readonly property color panelEdge: Qt.rgba(text.r, text.g, text.b, 0.07)
    readonly property color rule: Qt.rgba(text.r, text.g, text.b, 0.06)

    // Text
    readonly property color text: SystemTheme.colors["text.primary"]
    readonly property color textSoft: mix(text, textDim, 0.3)                  // sidebar places
    readonly property color textDim: SystemTheme.colors["text.secondary"]
    readonly property color textMute: SystemTheme.colors["text.tertiary"]
    readonly property color textDisabled: SystemTheme.colors["text.disabled"]
    readonly property color accentInk: SystemTheme.colors["text.onAccent"]

    // Accent and states: Files' own accent (chosen in Settings, else the theme's for Files).
    readonly property color accent: SystemTheme.accent
    readonly property color accentText: mix(accent, text, light ? 0 : 0.25)
    readonly property color selection: mix(panel, accent, light ? 0.14 : 0.18)
    readonly property color selectionEdge: Qt.rgba(accent.r, accent.g, accent.b, 0.55)
    readonly property color hover: mix(panel, accent, 0.07)
    readonly property color danger: SystemTheme.colors["status.error"]
    readonly property color warning: SystemTheme.colors["status.warning"]
    readonly property color success: SystemTheme.colors["status.success"]
    readonly property color folderIcon: mix(textDim, accent, 0.25)
    readonly property color dangerText: "#ffffff"
    readonly property color scrim: Qt.rgba(0, 0, 0, light ? 0.25 : 0.45)

    // Sidebar sections: a heading and its icons share the section's colour.
    readonly property color sectionFolders: accent
    readonly property color sectionPinned: SystemTheme.swatches["amber"]
    readonly property color sectionStorage: SystemTheme.swatches["violet"]

    // Swatches a sidebar section can take: the theme's own, by the names Settings keeps.
    readonly property var swatches: SystemTheme.swatches
    readonly property var swatchNames: Object.keys(SystemTheme.swatches)

    // A section's colour: its chosen swatch, else the theme's ("1" Folders, "2" Pinned, "3" Storage).
    function sectionColor(section, chosen) {
        const swatch = chosen ? chosen[String(section)] : undefined
        if (swatch !== undefined && swatches[swatch] !== undefined)
            return swatches[swatch]
        return section === 2 ? sectionPinned : section === 3 ? sectionStorage : sectionFolders
    }

    // Type: the theme's interface font at 13.5 px, scaled by the suite's text size.
    readonly property string family: SystemTheme.family
    readonly property string monoFamily: SystemTheme.monoFamily
    readonly property real scale: SystemTheme.textScale
    readonly property real body: 10.125 * scale         // points (13.5 px at 96 dpi)
    readonly property real small: 9 * scale             // 12 px
    readonly property real tiny: 8.625 * scale          // 11.5 px, headings and column titles
    readonly property real title: 10.875 * scale        // 14.5 px, the app name

    // Shape and rhythm, by the suite's density.
    readonly property bool compact: SystemTheme.density === "compact"
    readonly property bool relaxed: SystemTheme.density === "relaxed"
    readonly property int densityStep: compact ? -4 : relaxed ? 4 : 0
    readonly property int radius: SystemTheme.radiusSmall + 1
    readonly property int panelRadius: SystemTheme.radiusDialog - 1
    readonly property int headerHeight: 48 + densityStep
    readonly property int sidebarWidth: 229
    readonly property int placeHeight: Math.round((33 + densityStep) * Math.max(1, scale))
    readonly property int rowHeight: Math.round((30 + densityStep) * Math.max(1, scale))
    readonly property int controlSize: 32 + (compact ? -2 : relaxed ? 2 : 0)
    readonly property int iconSize: 16
    readonly property int railWidth: 56
}
