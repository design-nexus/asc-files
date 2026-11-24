// SPDX-License-Identifier: MIT
//! asc-files-render: decodes one image for Files, inside the sandbox Files runs it in, and
//! writes it out as a PNG no larger than asked. Untrusted image decoders never run in the app.
//!
//!     asc-files-render image <file> <max-pixels-per-side>

#include <QBuffer>
#include <QCoreApplication>
#include <QImage>
#include <QImageReader>
#include <QImageWriter>
#include <cstdio>
#include <unistd.h>

int main(int argc, char *argv[]) {
    QCoreApplication app(argc, argv);
    const QStringList args = app.arguments();
    if (args.size() != 4 || args.at(1) != QLatin1String("image")) {
        std::fputs("usage: asc-files-render image <file> <max>\n", stderr);
        return 2;
    }
    const int max = qBound(16, args.at(3).toInt(), 4096);
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
