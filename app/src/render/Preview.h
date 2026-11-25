// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QImage>
#include <QObject>
#include <QQuickImageProvider>
#include <QString>
#include <QVariantMap>

/// What the preview drawer shows for one item: an image or a PDF page (decoded in the sandbox),
/// text or code (the first 512 KB, read as UTF-8), Markdown (as text, or rendered), a folder's
/// summary, or the item's details alone.
class Preview : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString path READ path NOTIFY changed)
    Q_PROPERTY(QString name READ name NOTIFY changed)
    /// "image", "pdf", "text", "markdown", "folder", "other" or "" (nothing shown).
    Q_PROPERTY(QString kind READ kind NOTIFY changed)
    Q_PROPERTY(bool loading READ loading NOTIFY changed)
    Q_PROPERTY(QString error READ error NOTIFY changed)
    Q_PROPERTY(QString text READ text NOTIFY changed)
    /// A document's kind (`markdown`, `html`, `rtf`, `docx`, `sheet`, `csv`, `tsv`), its HTML as
    /// the document reader made it in the sandbox, and whether its source can be shown as text.
    Q_PROPERTY(QString documentKind READ documentKind NOTIFY changed)
    Q_PROPERTY(QString html READ html NOTIFY changed)
    Q_PROPERTY(bool hasSource READ hasSource NOTIFY changed)
    Q_PROPERTY(bool truncated READ truncated NOTIFY changed)
    /// What to ask the `image://preview/` provider for; changes with each image.
    Q_PROPERTY(QString imageSource READ imageSource NOTIFY changed)
    Q_PROPERTY(int page READ page NOTIFY changed)
    Q_PROPERTY(int pageCount READ pageCount NOTIFY changed)
    /// The facts shown under the preview: kind, size, modified, dimensions, pages.
    Q_PROPERTY(QVariantMap details READ details NOTIFY changed)
public:
    static constexpr qint64 MaxText = 512 * 1024;
    static constexpr int ImageSize = 1600;

    explicit Preview(QObject *parent = nullptr);
    ~Preview() override;

    /// Shows `path` (text, as a listing gives it); empty clears it.
    Q_INVOKABLE void show(const QString &path);
    Q_INVOKABLE void goToPage(int page);

    QString path() const { return m_path; }
    QString name() const;
    QString kind() const { return m_kind; }
    bool loading() const { return m_loading; }
    QString error() const { return m_error; }
    QString text() const { return m_text; }
    QString documentKind() const { return m_documentKind; }
    QString html() const { return m_html; }
    bool hasSource() const;
    /// The document kind of `path` (as the reader takes it), or empty when it isn't one.
    static QString documentKindOf(const QString &path);
    bool truncated() const { return m_truncated; }
    QString imageSource() const;
    int page() const { return m_page; }
    int pageCount() const { return m_pageCount; }
    QVariantMap details() const { return m_details; }
    QImage image() const { return m_image; }

    /// What a file is, for the preview, by its name and first bytes (read here: no decoder).
    static QString kindOf(const QString &path);

signals:
    void changed();

private:
    void loadImage();
    void loadDocument();
    void loadPdfPage();
    void loadText();
    void setDetails();

    QString m_path;
    QString m_kind;
    bool m_loading = false;
    QString m_error;
    QString m_text;
    bool m_truncated = false;
    QImage m_image;
    quint64 m_serial = 0;
    /// Which preview an image URL names: each window has its own.
    int m_id = 0;
    int m_page = 0;
    int m_pageCount = 0;
    QVariantMap m_details;
    QString m_documentKind;
    QString m_html;
    QObject *m_work = nullptr;
};

/// Serves each preview's current image as `image://preview/<preview>/<serial>`.
class PreviewImageProvider : public QQuickImageProvider {
public:
    PreviewImageProvider() : QQuickImageProvider(QQuickImageProvider::Image) {}
    QImage requestImage(const QString &id, QSize *size, const QSize &requestedSize) override;
};
