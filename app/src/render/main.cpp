// SPDX-License-Identifier: MIT
//! asc-files-render: decodes one image, or a frame of one video, for Files, inside the sandbox
//! Files runs it in, and writes it out as a PNG no larger than asked. Untrusted decoders never
//! run in the app.
//!
//!     asc-files-render image <file> <max-pixels-per-side>
//!     asc-files-render video <file> <max-pixels-per-side>   (a frame a tenth of the way in)

#include <QBuffer>
#include <QGuiApplication>
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <QMediaPlayer>
#include <QTimer>
#include <QVideoFrame>
#include <QVideoSink>
#include <cstdio>
#include <unistd.h>

namespace {

/// Writes `image` to stdout as a PNG, with `full` (the size before scaling) in it.
int writePng(QImage image, const QSize &full) {
    image.setText(QStringLiteral("Source::Size"), QStringLiteral("%1x%2").arg(full.width()).arg(full.height()));
    QBuffer out;
    out.open(QIODevice::WriteOnly);
    QImageWriter writer(&out, "png");
    writer.setText(QStringLiteral("Source::Size"), image.text(QStringLiteral("Source::Size")));
    if (!writer.write(image)) {
        return 1;
    }
    const QByteArray bytes = out.data();
    qsizetype written = 0;
    while (written < bytes.size()) {
        const ssize_t now = ::write(STDOUT_FILENO, bytes.constData() + written, size_t(bytes.size() - written));
        if (now <= 0) {
            return 1;
        }
        written += now;
    }
    return 0;
}

/// A frame a tenth of the way into the video `file` (at most 30 s in), silently: no sound is
/// played, only pictures decoded.
int videoFrame(QGuiApplication &app, const QString &file, int max) {
    QMediaPlayer player;
    QVideoSink sink;
    player.setVideoSink(&sink);
    qint64 wanted = 0;
    int result = 1;
    QObject::connect(&player, &QMediaPlayer::mediaStatusChanged, &app, [&](QMediaPlayer::MediaStatus status) {
        if (status == QMediaPlayer::LoadedMedia) {
            wanted = qMin(player.duration() / 10, qint64(30'000));
            if (wanted > 0) {
                player.setPosition(wanted);
            }
            player.play();
        } else if (status == QMediaPlayer::InvalidMedia) {
            app.exit(1);
        }
    });
    QObject::connect(&player, &QMediaPlayer::errorOccurred, &app, [&app] { app.exit(1); });
    QObject::connect(&sink, &QVideoSink::videoFrameChanged, &app, [&](const QVideoFrame &frame) {
        // Frames from before the place asked for come first, while it seeks.
        if (!frame.isValid() || frame.startTime() < (wanted - 1000) * 1000) {
            return;
        }
        QImage image = frame.toImage();
        if (image.isNull()) {
            return;
        }
        player.stop();
        const QSize full = image.size();
        if (image.width() > max || image.height() > max) {
            image = image.scaled(max, max, Qt::KeepAspectRatio, Qt::SmoothTransformation);
        }
        result = writePng(image.convertToFormat(QImage::Format_RGB32), full);
        app.exit(result);
    });
    QTimer::singleShot(10'000, &app, [&app] { app.exit(1); });
    player.setSource(QUrl::fromLocalFile(file));
    return app.exec();
}

} // namespace

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    const QStringList args = app.arguments();
    if (args.size() != 4 || (args.at(1) != QLatin1String("image") && args.at(1) != QLatin1String("video"))) {
        std::fputs("usage: asc-files-render image|video <file> <max>\n", stderr);
        return 2;
    }
    const int max = qBound(16, args.at(3).toInt(), 4096);
    if (args.at(1) == QLatin1String("video")) {
        return videoFrame(app, args.at(2), max);
    }
    QImageReader reader(args.at(2));
    reader.setAutoTransform(true);
    const QSize full = reader.size();
    // A picture that says it is enormous is not decoded.
    if (full.isValid() && qint64(full.width()) * full.height() > 200'000'000) {
        std::fputs("too large\n", stderr);
        return 3;
    }
    if (full.isValid() && (full.width() > max || full.height() > max)) {
        reader.setScaledSize(full.scaled(max, max, Qt::KeepAspectRatio));
    }
    QImage image = reader.read();
    if (image.isNull()) {
        std::fprintf(stderr, "%s\n", qPrintable(reader.errorString()));
        return 1;
    }
    if (image.width() > max || image.height() > max) {
        image = image.scaled(max, max, Qt::KeepAspectRatio, Qt::SmoothTransformation);
    }
    // The size before scaling travels in the PNG, for the preview's details.
    return writePng(image, full);
}
