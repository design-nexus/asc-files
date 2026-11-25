// SPDX-License-Identifier: MIT
#include "Thumbnails.h"

#include "../render/Sandbox.h"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QPointer>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImageReader>
#include <QImageWriter>
#include <QSaveFile>
#include <QStandardPaths>
#include <QThreadPool>
#include <QUrl>
#include <sys/stat.h>

namespace Thumbnails {

namespace {

QString cacheRoot() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation) + QStringLiteral("/thumbnails");
}

QByteArray uriOf(const QByteArray &path) { return QUrl::fromLocalFile(QFile::decodeName(path)).toEncoded(); }

QString hashOf(const QByteArray &path) {
    return QString::fromLatin1(QCryptographicHash::hash(uriOf(path), QCryptographicHash::Md5).toHex());
}

QString failPath(const QByteArray &path) {
    return cacheRoot() + QStringLiteral("/fail/asc-files/") + hashOf(path) + QStringLiteral(".png");
}

/// Writes `image` as the thumbnail of `path`, with the keys the standard asks for.
void keep(QImage image, const QString &where, const QByteArray &path, qint64 mtime) {
    image.setText(QStringLiteral("Thumb::URI"), QString::fromLatin1(uriOf(path)));
    image.setText(QStringLiteral("Thumb::MTime"), QString::number(mtime));
    image.setText(QStringLiteral("Software"), QStringLiteral("Files (asc-files)"));
    QDir().mkpath(QFileInfo(where).path());
    // Written whole and renamed into place, as the standard asks, with only the owner allowed in.
    QSaveFile file(where);
    if (!file.open(QIODevice::WriteOnly)) {
        return;
    }
    QImageWriter writer(&file, "png");
    for (const QString &key : image.textKeys()) {
        writer.setText(key, image.text(key));
    }
    if (writer.write(image)) {
        file.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner);
        file.commit();
    }
}

/// The cached thumbnail at `where`, if it was made from the file as it is now.
QImage cached(const QString &where, qint64 mtime) {
    // The keys are known once the image is read (they may follow its data).
    QImage image;
    if (!image.load(where, "png") || image.text(QStringLiteral("Thumb::MTime")) != QString::number(mtime)) {
        return {};
    }
    return image;
}

} // namespace

int cacheSize(int pixels) { return pixels > 128 ? 256 : 128; }

QString cachePath(const QByteArray &path, int size) {
    return cacheRoot() + (size > 128 ? QStringLiteral("/large/") : QStringLiteral("/normal/")) + hashOf(path) + QStringLiteral(".png");
}

namespace {

enum class Source { None, Picture, Video, Pdf };

Source sourceOf(const QString &name) {
    static const QList<QByteArray> formats = QImageReader::supportedImageFormats();
    static const QList<QByteArray> videos{"mp4", "m4v", "mkv", "webm", "mov", "avi", "ogv", "mpg", "mpeg", "wmv", "flv", "3gp"};
    const QByteArray suffix = QFileInfo(name).suffix().toLower().toLatin1();
    if (suffix.isEmpty()) {
        return Source::None;
    }
    if (formats.contains(suffix == "jpeg" ? QByteArray("jpg") : suffix)) {
        return Source::Picture;
    }
    if (videos.contains(suffix)) {
        return Source::Video;
    }
    return suffix == "pdf" ? Source::Pdf : Source::None;
}

} // namespace

bool canMake(const QString &name) { return sourceOf(name) != Source::None; }

QImage thumbnail(const QByteArray &path, int size) {
    struct stat st {};
    const Source source = sourceOf(QFile::decodeName(path));
    // Regular local files only, never through a link or a device or pipe. A video's frame is
    // read without reading it all, so its size doesn't count.
    if (::lstat(path.constData(), &st) != 0 || !S_ISREG(st.st_mode) || (source != Source::Video && st.st_size > MaxSourceBytes)) {
        return {};
    }
    const qint64 mtime = st.st_mtime;
    const int wanted = cacheSize(size);
    const QString where = cachePath(path, wanted);
    if (QImage image = cached(where, mtime); !image.isNull()) {
        return image;
    }
    // Tried before and failed: tried again only once the file has changed.
    if (!cached(failPath(path), mtime).isNull()) {
        return {};
    }
    // Decoded in the sandbox, as every untrusted file is.
    QImage image = source == Source::Video ? Sandbox::videoFrameNow(path, wanted)
                   : source == Source::Pdf ? Sandbox::pdfPageNow(path, wanted)
                                           : Sandbox::decodeImageNow(path, wanted);
    if (image.isNull()) {
        QImage marker(1, 1, QImage::Format_ARGB32);
        marker.fill(Qt::transparent);
        keep(marker, failPath(path), path, mtime);
        return {};
    }
    if (image.width() > wanted || image.height() > wanted) {
        image = image.scaled(wanted, wanted, Qt::KeepAspectRatio, Qt::SmoothTransformation);
    }
    keep(image, where, path, mtime);
    return image;
}

} // namespace Thumbnails

namespace {

class Response : public QQuickImageResponse {
public:
    QQuickTextureFactory *textureFactory() const override { return QQuickTextureFactory::textureFactoryForImage(m_image); }
    QString errorString() const override { return m_image.isNull() ? QStringLiteral("no thumbnail") : QString(); }

    void deliver(const QImage &image) {
        m_image = image;
        emit finished();
    }

private:
    QImage m_image;
};

} // namespace

QQuickImageResponse *ThumbnailProvider::requestImageResponse(const QString &id, const QSize &requestedSize) {
    const QByteArray path = QByteArray::fromPercentEncoding(id.toLatin1());
    const int size = qMax(requestedSize.width(), requestedSize.height());
    auto *response = new Response;
    // The view may let the response go before the image is made: the worker hands the image to
    // the main thread, which looks there whether the response is still wanted.
    QPointer<Response> wanted(response);
    QThreadPool::globalInstance()->start([wanted, path, size] {
        const QImage image = Thumbnails::thumbnail(path, size);
        QMetaObject::invokeMethod(
            QCoreApplication::instance(),
            [wanted, image] {
                if (wanted) {
                    wanted->deliver(image);
                }
            },
            Qt::QueuedConnection);
    });
    return response;
}
