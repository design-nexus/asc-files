// SPDX-License-Identifier: MIT
#include "Preview.h"

#include "Sandbox.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QLocale>
#include <QMimeDatabase>
#include <QMutex>
#include <QStringDecoder>

namespace {

/// Markdown and plain text by name; source code by its MIME type being text.
bool isMarkdown(const QString &path) {
    const QString suffix = QFileInfo(path).suffix().toLower();
    return suffix == QLatin1String("md") || suffix == QLatin1String("markdown");
}

} // namespace

namespace {

/// The previews alive, by id, for the image provider (which the QML engine calls on its own
/// thread).
QMutex previewsLock;
QHash<int, Preview *> previews;
int nextPreview = 1;

} // namespace

Preview::Preview(QObject *parent) : QObject(parent) {
    const QMutexLocker lock(&previewsLock);
    m_id = nextPreview++;
    previews.insert(m_id, this);
}

Preview::~Preview() {
    const QMutexLocker lock(&previewsLock);
    previews.remove(m_id);
}

QString Preview::name() const { return QFileInfo(m_path).fileName(); }

QString Preview::imageSource() const {
    return m_image.isNull() ? QString() : QStringLiteral("image://preview/%1/%2").arg(m_id).arg(m_serial);
}

QString Preview::kindOf(const QString &path) {
    const QFileInfo info(path);
    if (info.isDir()) {
        return QStringLiteral("folder");
    }
    if (!info.isFile()) {
        return QStringLiteral("other");
    }
    static QMimeDatabase mimes;
    // By name and the first bytes, which only reads the file.
    const QMimeType type = mimes.mimeTypeForFile(path);
    const QString name = type.name();
    if (name == QLatin1String("application/pdf")) {
        return QStringLiteral("pdf");
    }
    if (name.startsWith(QLatin1String("image/")) && name != QLatin1String("image/svg+xml")) {
        return QStringLiteral("image");
    }
    // Played by the preview (Qt Multimedia), as legacy played them.
    if (name.startsWith(QLatin1String("audio/"))) {
        return QStringLiteral("audio");
    }
    if (name.startsWith(QLatin1String("video/"))) {
        return QStringLiteral("video");
    }
    if (isMarkdown(path)) {
        return QStringLiteral("markdown");
    }
    if (type.inherits(QStringLiteral("text/plain")) || name == QLatin1String("image/svg+xml") || name == QLatin1String("application/json") ||
        name == QLatin1String("application/x-shellscript")) {
        return QStringLiteral("text");
    }
    return QStringLiteral("other");
}

void Preview::show(const QString &path) {
    if (path == m_path) {
        return;
    }
    // Whatever was being decoded for the last item is let go.
    delete m_work;
    m_work = new QObject(this);
    m_path = path;
    m_kind = path.isEmpty() ? QString() : kindOf(path);
    m_error.clear();
    m_text.clear();
    m_truncated = false;
    m_image = {};
    m_page = 0;
    m_pageCount = 0;
    m_loading = false;
    setDetails();
    if (m_kind == QLatin1String("image")) {
        loadImage();
    } else if (m_kind == QLatin1String("pdf")) {
        m_loading = true;
        const QString pdfinfo = Sandbox::program(QStringLiteral("pdfinfo"));
        if (pdfinfo.isEmpty()) {
            m_loading = false;
            m_error = tr("PDF previews need poppler-utils");
        } else {
            Sandbox::run(pdfinfo, {QStringLiteral("{}")}, QFile::encodeName(path), m_work, [this](const QByteArray &out, const QString &error) {
                if (!error.isEmpty()) {
                    m_loading = false;
                    m_error = error;
                    emit changed();
                    return;
                }
                for (const QByteArray &line : out.split('\n')) {
                    if (line.startsWith("Pages:")) {
                        m_pageCount = line.mid(6).trimmed().toInt();
                    }
                }
                m_details.insert(QStringLiteral("pages"), m_pageCount);
                m_page = 1;
                loadPdfPage();
            });
        }
    } else if (m_kind == QLatin1String("text") || m_kind == QLatin1String("markdown")) {
        loadText();
    }
    emit changed();
}

void Preview::goToPage(int page) {
    if (m_kind != QLatin1String("pdf") || page < 1 || page > m_pageCount || page == m_page) {
        return;
    }
    m_page = page;
    delete m_work;
    m_work = new QObject(this);
    loadPdfPage();
    emit changed();
}

void Preview::loadImage() {
    m_loading = true;
    Sandbox::decodeImage(QFile::encodeName(m_path), ImageSize, m_work, [this](const QImage &image, const QString &error) {
        m_loading = false;
        m_error = error;
        m_image = image;
        ++m_serial;
        const QString size = image.text(QStringLiteral("Source::Size"));
        if (!size.isEmpty() && !size.startsWith(u'-')) {
            m_details.insert(QStringLiteral("dimensions"), QString(size).replace(u'x', QStringLiteral(" × ")));
        }
        emit changed();
    });
}

void Preview::loadPdfPage() {
    m_loading = true;
    const QString pdftoppm = Sandbox::program(QStringLiteral("pdftoppm"));
    Sandbox::run(pdftoppm,
                 {QStringLiteral("-png"), QStringLiteral("-f"), QString::number(m_page), QStringLiteral("-l"), QString::number(m_page),
                  QStringLiteral("-scale-to"), QString::number(ImageSize), QStringLiteral("-singlefile"), QStringLiteral("{}")},
                 QFile::encodeName(m_path), m_work, [this](const QByteArray &out, const QString &error) {
                     m_loading = false;
                     m_error = error;
                     m_image.loadFromData(out, "PNG");
                     if (m_image.isNull() && m_error.isEmpty()) {
                         m_error = tr("The page could not be shown");
                     }
                     ++m_serial;
                     emit changed();
                 });
}

void Preview::loadText() {
    QFile file(m_path);
    if (!file.open(QIODevice::ReadOnly)) {
        m_error = file.errorString();
        return;
    }
    const QByteArray bytes = file.read(MaxText + 1);
    m_truncated = bytes.size() > MaxText;
    const QByteArray shown = bytes.left(MaxText);
    // Binary content (a NUL early on) is not text, whatever its name.
    if (shown.left(8192).contains('\0')) {
        m_kind = QStringLiteral("other");
        return;
    }
    QStringDecoder decoder(QStringDecoder::Utf8);
    m_text = decoder.decode(shown);
}

void Preview::setDetails() {
    m_details.clear();
    if (m_path.isEmpty()) {
        return;
    }
    const QFileInfo info(m_path);
    static QMimeDatabase mimes;
    m_details.insert(QStringLiteral("kind"), info.isDir() ? tr("Folder") : mimes.mimeTypeForFile(m_path).comment());
    if (info.isDir()) {
        m_details.insert(QStringLiteral("items"), QDir(m_path).entryList(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden).size());
    } else {
        m_details.insert(QStringLiteral("size"), QLocale().formattedDataSize(info.size(), 1, QLocale::DataSizeSIFormat));
    }
    m_details.insert(QStringLiteral("modified"), QLocale().toString(info.lastModified(), QLocale::ShortFormat));
    m_details.insert(QStringLiteral("where"), info.path());
}

QImage PreviewImageProvider::requestImage(const QString &id, QSize *size, const QSize &) {
    QImage image;
    {
        const QMutexLocker lock(&previewsLock);
        if (const Preview *preview = previews.value(id.section(u'/', 0, 0).toInt())) {
            image = preview->image();
        }
    }
    if (size) {
        *size = image.size();
    }
    return image;
}
