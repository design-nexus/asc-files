// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QImage>
#include <QQuickAsyncImageProvider>
#include <QString>

/// Thumbnails, by the freedesktop.org Thumbnail Managing Standard: a thumbnail is a PNG named by
/// the MD5 of the file's URI in `$XDG_CACHE_HOME/thumbnails/{normal,large}`, carrying the file's
/// URI and modified time, so one made by any program is used by every other and a changed file
/// gets a new one. A file that could not be read is noted under `fail/asc-files` and not tried
/// again until it changes.
///
/// Files makes thumbnails of images Qt can read, from local files up to 64 MB, off the window's
/// thread. The images are decoded in this process for now; decoding in a sandbox comes with the
/// preview's renderers (B3).
namespace Thumbnails {

/// The largest file Files reads to make a thumbnail.
constexpr qint64 MaxSourceBytes = 64 * 1024 * 1024;

/// The cache's size for `pixels`: 128 (normal) or 256 (large).
int cacheSize(int pixels);
/// Where the thumbnail of `path` (raw bytes) at `size` is kept.
QString cachePath(const QByteArray &path, int size);
/// Whether Files would try to make a thumbnail for `name` at all (an image Qt can read).
bool canMake(const QString &name);
/// The thumbnail of `path` no larger than `size`: from the cache when it matches the file, else
/// made and kept. Null when there is none to make, or the file could not be read.
QImage thumbnail(const QByteArray &path, int size);

} // namespace Thumbnails

/// Serves `image://thumbnail/<percent-encoded raw path>` to QML, each on a worker thread.
class ThumbnailProvider : public QQuickAsyncImageProvider {
public:
    QQuickImageResponse *requestImageResponse(const QString &id, const QSize &requestedSize) override;
};
