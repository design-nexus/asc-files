// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Shapes

import AscFiles

// A stroke icon drawn from SVG path data on a 16×16 grid, in the line style Files uses.
Item {
    id: icon

    property string path
    property color color: Theme.textDim
    property color fill: "transparent"
    property real strokeWidth: 1.4
    property int size: Theme.iconSize

    implicitWidth: size
    implicitHeight: size

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        transform: Scale { xScale: icon.size / 16; yScale: icon.size / 16 }

        ShapePath {
            strokeColor: icon.color
            strokeWidth: icon.strokeWidth
            fillColor: icon.fill
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg { path: icon.path }
        }
    }
}
