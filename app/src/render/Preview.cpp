// SPDX-License-Identifier: MIT
#include "Preview.h"

#include "Sandbox.h"

#include <algorithm>

#include <QColor>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocale>
#include <QMimeDatabase>
#include <QMutex>
#include <QRegularExpression>
#include <QStringDecoder>

namespace {

/// Markdown and plain text by name; source code by its MIME type being text.

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

QString Preview::documentKindOf(const QString &path) {
    static QMimeDatabase mimes;
    const QString type = mimes.mimeTypeForFile(path).name();
    const QString suffix = QFileInfo(path).suffix().toLower();
    if (type == QLatin1String("text/markdown") || QStringList{"md", "markdown", "mdown", "mkd", "mkdn"}.contains(suffix)) {
        return QStringLiteral("markdown");
    }
    if (type == QLatin1String("text/html") || type == QLatin1String("application/xhtml+xml")) {
        return QStringLiteral("html");
    }
    if (type == QLatin1String("application/rtf") || type == QLatin1String("text/rtf") || suffix == QLatin1String("rtf")) {
        return QStringLiteral("rtf");
    }
    if (type == QLatin1String("application/vnd.openxmlformats-officedocument.wordprocessingml.document") || suffix == QLatin1String("docx")) {
        return QStringLiteral("docx");
    }
    if (type == QLatin1String("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet") ||
        type == QLatin1String("application/vnd.ms-excel") || type == QLatin1String("application/vnd.oasis.opendocument.spreadsheet") ||
        QStringList{"xlsx", "xls", "ods"}.contains(suffix)) {
        return QStringLiteral("sheet");
    }
    if (type == QLatin1String("text/csv") || suffix == QLatin1String("csv")) {
        return QStringLiteral("csv");
    }
    if (type == QLatin1String("text/tab-separated-values") || suffix == QLatin1String("tsv")) {
        return QStringLiteral("tsv");
    }
    return {};
}

bool Preview::hasSource() const {
    return QStringList{"markdown", "html", "rtf", "csv", "tsv"}.contains(m_documentKind);
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
    if (!documentKindOf(path).isEmpty()) {
        return QStringLiteral("document");
    }
    // By name only: Word documents, e-books and comics are ZIP files too, with kinds of their own.
    static const QStringList archives{QStringLiteral(".zip"), QStringLiteral(".7z"), QStringLiteral(".tar"),
                                      QStringLiteral(".tar.gz"), QStringLiteral(".tgz"), QStringLiteral(".rar")};
    const QString lower = info.fileName().toLower();
    if (std::any_of(archives.begin(), archives.end(), [&](const QString &suffix) { return lower.endsWith(suffix); })) {
        return QStringLiteral("archive");
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
    m_documentKind = m_kind == QLatin1String("document") ? documentKindOf(path) : QString();
    m_html.clear();
    m_archive.clear();
    m_media.clear();
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
    } else if (m_kind == QLatin1String("text")) {
        loadText();
    } else if (m_kind == QLatin1String("archive")) {
        loadArchive();
    } else if (m_kind == QLatin1String("document")) {
        loadDocument();
        if (hasSource()) {
            loadText();
        }
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

void Preview::setInk(const QString &ink) {
    if (ink != m_ink && QColor::isValidColorName(ink)) {
        m_ink = ink;
        emit changed();
    }
}

void Preview::loadDocument() {
    m_loading = true;
    Sandbox::run(Sandbox::documentReader(), {m_documentKind, QStringLiteral("{}"), m_ink}, QFile::encodeName(m_path), m_work,
                 [this](const QByteArray &out, const QString &error) {
                     m_loading = false;
                     const QJsonObject read = QJsonDocument::fromJson(out).object();
                     m_html = read.value(QStringLiteral("html")).toString();
                     // Pictures it drew, served by the image provider: `media:N` names them.
                     for (const QJsonValue &png : read.value(QStringLiteral("media")).toArray()) {
                         m_media.append(QImage::fromData(QByteArray::fromBase64(png.toString().toLatin1()), "PNG"));
                     }
                     ++m_serial;
                     static const QRegularExpression named(QStringLiteral("src=\"media:(\\d+)\""));
                     m_html.replace(named, QStringLiteral("src=\"image://preview/%1/media/\\1-%2\"").arg(m_id).arg(m_serial));
                     m_truncated = m_truncated || read.value(QStringLiteral("truncated")).toBool();
                     m_error = !error.isEmpty() ? error : read.isEmpty() ? tr("The document could not be read") : QString();
                     emit changed();
                 });
}

void Preview::loadArchive() {
    m_loading = true;
    // Its format by its name, which the reader can't see in the sandbox.
    const QString lower = QFileInfo(m_path).fileName().toLower();
    const QString format = lower.endsWith(QLatin1String(".tar.gz")) || lower.endsWith(QLatin1String(".tgz"))
                               ? QStringLiteral("tar.gz")
                               : QFileInfo(lower).suffix();
    Sandbox::run(Sandbox::documentReader(), {QStringLiteral("archive"), format, QStringLiteral("{}")}, QFile::encodeName(m_path), m_work,
                 [this](const QByteArray &out, const QString &error) {
                     m_loading = false;
                     m_archive = QJsonDocument::fromJson(out).object().toVariantMap();
                     m_error = !error.isEmpty() ? error : m_archive.isEmpty() ? tr("The archive could not be read") : QString();
                     if (m_archive.contains(QStringLiteral("files"))) {
                         m_details.insert(QStringLiteral("items"), m_archive.value(QStringLiteral("files")));
                     }
                     emit changed();
                 });
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
            image = id.section(u'/', 1, 1) == QLatin1String("media") ? preview->media(id.section(u'/', 2, 2).section(u'-', 0, 0).toInt())
                                                                    : preview->image();
        }
    }
    if (size) {
        *size = image.size();
    }
    return image;
}
