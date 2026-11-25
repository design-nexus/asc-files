// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtMultimedia

import AscFiles
import AscFiles.System

// Audio and video in the preview, played as legacy's preview played them: the picture (or a
// note for sound), then play and pause, where it is, and the volume (kept). It starts by itself
// only when Settings says to; it stops when the preview moves on or closes, as it goes with it.
ColumnLayout {
    id: media

    property string path
    property bool video: false

    spacing: 10

    function time(ms) {
        const seconds = Math.floor(ms / 1000)
        const minutes = Math.floor(seconds / 60)
        const hours = Math.floor(minutes / 60)
        const two = (n) => (n < 10 ? "0" : "") + n
        return hours > 0 ? hours + ":" + two(minutes % 60) + ":" + two(seconds % 60) : minutes + ":" + two(seconds % 60)
    }

    MediaPlayer {
        id: player
        objectName: "mediaPlayer"
        source: media.path.length > 0 ? "file://" + media.path.split("/").map(encodeURIComponent).join("/") : ""
        audioOutput: AudioOutput { volume: AppSettings.mediaVolume }
        videoOutput: output
        onMediaStatusChanged: if (mediaStatus === MediaPlayer.LoadedMedia && AppSettings.autoplayMedia && playbackState !== MediaPlayer.PlayingState) play()
    }

    Item {
        Layout.fillWidth: true
        Layout.fillHeight: true
        VideoOutput {
            id: output
            anchors.fill: parent
            visible: media.video
            fillMode: VideoOutput.PreserveAspectFit
        }
        Icon {
            visible: !media.video
            anchors.centerIn: parent
            size: 96
            strokeWidth: 0.8
            color: Theme.folderIcon
            path: "M6 12V3.5l7-1.5v8.5M6 12a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM13 10.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0z"
        }
        TapHandler { enabled: media.video; onTapped: player.playing ? player.pause() : player.play() }
        // Until it plays, a video shows nothing: a play button stands in its place.
        Rectangle {
            visible: media.video && !player.playing && player.error === MediaPlayer.NoError
            anchors.centerIn: parent
            width: 64; height: 64; radius: 32
            color: Qt.rgba(0, 0, 0, 0.45)
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, 0.25)
            Icon { anchors.centerIn: parent; anchors.horizontalCenterOffset: 2; path: "M5 3.5v9l7-4.5z"; size: 28; color: "white"; strokeWidth: 1.4 }
        }
    }

    Text {
        visible: player.error !== MediaPlayer.NoError
        Layout.fillWidth: true
        text: qsTr("This can't be played: %1").arg(player.errorString)
        wrapMode: Text.Wrap
        color: Theme.danger
        font.family: Theme.family
        font.pointSize: Theme.small
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        IconButton {
            objectName: "playButton"
            width: 30; height: 30
            enabled: player.error === MediaPlayer.NoError && player.mediaStatus !== MediaPlayer.NoMedia
            label: player.playing ? qsTr("Pause") : qsTr("Play")
            iconPath: player.playing ? "M5 3.5v9M11 3.5v9" : "M5 3.5v9l7-4.5z"
            onClicked: player.playing ? player.pause() : player.play()
        }
        Text {
            text: media.time(player.position)
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.small
            font.features: { "tnum": 1 }
        }
        Slider {
            id: where
            Layout.fillWidth: true
            from: 0
            to: Math.max(1, player.duration)
            value: player.position
            enabled: player.seekable
            onMoved: player.position = value
            Accessible.name: qsTr("Position")
            background: Rectangle {
                x: where.leftPadding
                y: where.topPadding + where.availableHeight / 2 - height / 2
                width: where.availableWidth
                height: 4
                radius: 2
                color: Theme.rule
                Rectangle { width: where.visualPosition * parent.width; height: parent.height; radius: 2; color: Theme.accent }
            }
            handle: Rectangle {
                x: where.leftPadding + where.visualPosition * (where.availableWidth - width)
                y: where.topPadding + where.availableHeight / 2 - height / 2
                width: 12; height: 12; radius: 6
                color: Theme.text
            }
        }
        Text {
            text: media.time(player.duration)
            color: Theme.textMute
            font.family: Theme.family
            font.pointSize: Theme.small
            font.features: { "tnum": 1 }
        }
        Icon {
            path: "M2.5 6h2.5l3.5-3v10l-3.5-3H2.5zM11 5.5a3.5 3.5 0 0 1 0 5"
            size: 14
            color: Theme.textMute
        }
        Slider {
            id: loudness
            Layout.preferredWidth: 80
            from: 0
            to: 1
            value: AppSettings.mediaVolume
            onMoved: AppSettings.mediaVolume = value
            Accessible.name: qsTr("Volume")
            background: Rectangle {
                x: loudness.leftPadding
                y: loudness.topPadding + loudness.availableHeight / 2 - height / 2
                width: loudness.availableWidth
                height: 4
                radius: 2
                color: Theme.rule
                Rectangle { width: loudness.visualPosition * parent.width; height: parent.height; radius: 2; color: Theme.textDim }
            }
            handle: Rectangle {
                x: loudness.leftPadding + loudness.visualPosition * (loudness.availableWidth - width)
                y: loudness.topPadding + loudness.availableHeight / 2 - height / 2
                width: 10; height: 10; radius: 5
                color: Theme.text
            }
        }
    }
}
