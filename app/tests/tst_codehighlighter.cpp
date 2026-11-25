// SPDX-License-Identifier: MIT
#include "render/CodeHighlighter.h"

#include <QTest>

class TestCodeHighlighter : public QObject {
    Q_OBJECT
private slots:
    void theLanguageComesFromTheName() {
        QCOMPARE(CodeHighlighter::languageFor("main.cpp"), QStringLiteral("C++"));
        QCOMPARE(CodeHighlighter::languageFor("tool.py"), QStringLiteral("Python"));
        QCOMPARE(CodeHighlighter::languageFor("lib.rs"), QStringLiteral("Rust"));
        QCOMPARE(CodeHighlighter::languageFor("Main.qml"), QStringLiteral("QML"));
        QCOMPARE(CodeHighlighter::languageFor("CMakeLists.txt"), QStringLiteral("CMake"));
        QCOMPARE(CodeHighlighter::languageFor("notes.nothing-knows-this"), QString());
    }
};

QTEST_GUILESS_MAIN(TestCodeHighlighter)
#include "tst_codehighlighter.moc"
