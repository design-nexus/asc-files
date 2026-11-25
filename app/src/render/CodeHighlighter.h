// SPDX-License-Identifier: MIT
#pragma once

#include <QObject>
#include <QPointer>
#include <QQuickTextDocument>

namespace KSyntaxHighlighting {
class SyntaxHighlighter;
}

/// Colours a preview's text as its language reads, by its file name (KDE's syntax definitions,
/// in place of legacy's GtkSourceView), in a light or dark theme to suit Files'. Text past
/// `MaxBytes` stays plain: highlighting runs where the window draws.
class CodeHighlighter : public QObject {
    Q_OBJECT
    Q_PROPERTY(QQuickTextDocument *document READ document WRITE setDocument NOTIFY changed)
    Q_PROPERTY(QString fileName READ fileName WRITE setFileName NOTIFY changed)
    Q_PROPERTY(bool dark READ dark WRITE setDark NOTIFY changed)
    /// The language found ("C++", "Python"); empty when none or not highlighted.
    Q_PROPERTY(QString language READ language NOTIFY changed)
public:
    static constexpr qsizetype MaxBytes = 200 * 1024;

    explicit CodeHighlighter(QObject *parent = nullptr);
    ~CodeHighlighter() override;

    QQuickTextDocument *document() const { return m_document; }
    void setDocument(QQuickTextDocument *document);
    QString fileName() const { return m_fileName; }
    void setFileName(const QString &name);
    bool dark() const { return m_dark; }
    void setDark(bool dark);
    QString language() const { return m_language; }

    /// The language a file name says, as the definitions name it; empty for none.
    static QString languageFor(const QString &fileName);

signals:
    void changed();

private:
    void update();

    QPointer<QQuickTextDocument> m_document;
    QString m_fileName;
    bool m_dark = true;
    QString m_language;
    /// Its document owns it, and may go first.
    QPointer<KSyntaxHighlighting::SyntaxHighlighter> m_highlighter;
    /// Colouring changes the document, which asks for an update: not while one runs.
    bool m_updating = false;
};
