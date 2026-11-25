// SPDX-License-Identifier: MIT
#include "CodeHighlighter.h"

#include <KSyntaxHighlighting/Definition>
#include <KSyntaxHighlighting/Repository>
#include <KSyntaxHighlighting/SyntaxHighlighter>
#include <KSyntaxHighlighting/Theme>
#include <QScopedValueRollback>
#include <QTextDocument>

namespace {

KSyntaxHighlighting::Repository &repository() {
    static KSyntaxHighlighting::Repository definitions;
    return definitions;
}

} // namespace

CodeHighlighter::CodeHighlighter(QObject *parent) : QObject(parent) {}

CodeHighlighter::~CodeHighlighter() = default;

void CodeHighlighter::setDocument(QQuickTextDocument *document) {
    if (document != m_document) {
        m_document = document;
        if (m_document) {
            // The text arrives after the file name: colour it once it has.
            connect(m_document->textDocument(), &QTextDocument::contentsChanged, this, &CodeHighlighter::update, Qt::UniqueConnection);
        }
        update();
    }
}

void CodeHighlighter::setFileName(const QString &name) {
    if (name != m_fileName) {
        m_fileName = name;
        update();
    }
}

void CodeHighlighter::setDark(bool dark) {
    if (dark != m_dark) {
        m_dark = dark;
        update();
    }
}

QString CodeHighlighter::languageFor(const QString &fileName) {
    const KSyntaxHighlighting::Definition definition = repository().definitionForFileName(fileName);
    return definition.isValid() ? definition.name() : QString();
}

void CodeHighlighter::update() {
    if (m_updating) {
        return;
    }
    const QScopedValueRollback updating(m_updating, true);
    QTextDocument *text = m_document ? m_document->textDocument() : nullptr;
    const KSyntaxHighlighting::Definition definition = repository().definitionForFileName(m_fileName);
    const bool wanted = text && definition.isValid() && text->characterCount() <= MaxBytes;
    const QString language = wanted ? definition.name() : QString();
    if (!wanted) {
        delete std::exchange(m_highlighter, nullptr).data();
    } else {
        if (!m_highlighter || m_highlighter->document() != text) {
            delete std::exchange(m_highlighter, nullptr).data();
            m_highlighter = new KSyntaxHighlighting::SyntaxHighlighter(text);
        }
        const auto theme = repository().defaultTheme(m_dark ? KSyntaxHighlighting::Repository::DarkTheme
                                                            : KSyntaxHighlighting::Repository::LightTheme);
        if (m_highlighter->definition() != definition || m_highlighter->theme().name() != theme.name()) {
            m_highlighter->setTheme(theme);
            m_highlighter->setDefinition(definition);
        }
    }
    if (language != m_language) {
        m_language = language;
        emit changed();
    }
}
