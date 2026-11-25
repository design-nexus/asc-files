// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "render/Preview.h"
#include "render/Sandbox.h"

#include <QFile>
#include <QImage>
#include <QPainter>
#include <QPdfWriter>
#include <QSignalSpy>
#include <QTemporaryDir>

namespace {

void write(const QString &path, const QByteArray &contents) {
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
}

bool settled(Preview &preview) {
    return QTest::qWaitFor([&] { return !preview.loading(); }, 20000);
}

} // namespace

class TestPreview : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        qInfo() << "sandboxed:" << Sandbox::sandboxed() << "renderer:" << Sandbox::renderer();
        QVERIFY(!Sandbox::renderer().isEmpty());
    }

    void anImageIsDecodedInTheSandbox() {
        QTemporaryDir dir;
        QImage image(3000, 1000, QImage::Format_RGB32);
        image.fill(Qt::darkCyan);
        QVERIFY(image.save(dir.filePath("wide.png")));
        Preview preview;
        preview.show(dir.filePath("wide.png"));
        QCOMPARE(preview.kind(), QStringLiteral("image"));
        QVERIFY(settled(preview));
        QVERIFY2(preview.error().isEmpty(), qPrintable(preview.error()));
        QCOMPARE(preview.image().size(), QSize(1600, 533));
        QCOMPARE(preview.image().pixelColor(10, 10), QColor(Qt::darkCyan));
        QCOMPARE(preview.details().value("dimensions").toString(), QStringLiteral("3000 × 1000"));
        QVERIFY(preview.imageSource().startsWith("image://preview/"));
    }

    void aBrokenImageSaysSoAndTheAppGoesOn() {
        QTemporaryDir dir;
        write(dir.filePath("broken.png"), QByteArray("\x89PNG\r\n\x1a\n", 8) + QByteArray(64, 'x'));
        Preview preview;
        preview.show(dir.filePath("broken.png"));
        QVERIFY(settled(preview));
        QVERIFY(!preview.error().isEmpty());
        QVERIFY(preview.image().isNull());
    }

    void textMarkdownAndBinary() {
        QTemporaryDir dir;
        write(dir.filePath("code.rs"), "fn main() {}\n");
        write(dir.filePath("notes.md"), "# Title\n\nSome *text*.\n");
        write(dir.filePath("data.txt"), QByteArray("abc\0def", 7));
        write(dir.filePath("big.txt"), QByteArray(Preview::MaxText + 100, 'a'));
        Preview preview;
        preview.show(dir.filePath("code.rs"));
        QCOMPARE(preview.kind(), QStringLiteral("text"));
        QCOMPARE(preview.text(), QStringLiteral("fn main() {}\n"));
        // Documents are read by the document reader, in the sandbox, as HTML that reaches nothing;
        // the source of text ones stays to show.
        preview.show(dir.filePath("notes.md"));
        QCOMPARE(preview.kind(), QStringLiteral("document"));
        QCOMPARE(preview.documentKind(), QStringLiteral("markdown"));
        QVERIFY(preview.hasSource());
        QTRY_VERIFY(preview.text().startsWith("# Title"));
        QTRY_VERIFY_WITH_TIMEOUT(preview.html().contains("<h1>Title</h1>"), 10000);
        QVERIFY(preview.html().contains("<em>text</em>"));
        // Diagrams and equations are drawn by the reader, and served by the preview.
        write(dir.filePath("maths.md"), "Energy: $E=mc^2$\n\n```mermaid\nflowchart LR\n A --> B\n```\n");
        preview.show(dir.filePath("maths.md"));
        QTRY_VERIFY_WITH_TIMEOUT(preview.html().contains("image://preview/"), 20000);
        QVERIFY(!preview.html().contains("media:0\""));
        QVERIFY(!preview.media(0).isNull());
        QVERIFY(!preview.media(1).isNull());
        QVERIFY(preview.media(2).isNull());
        write(dir.filePath("page.html"), "<html><body><h2>Hi</h2><img src=\"https://example.com/t.png\"><script>x()</script></body></html>");
        preview.show(dir.filePath("page.html"));
        QCOMPARE(preview.documentKind(), QStringLiteral("html"));
        QTRY_VERIFY_WITH_TIMEOUT(preview.html().contains("<h2>Hi</h2>"), 10000);
        QVERIFY(!preview.html().contains("example.com") && !preview.html().contains("script"));
        write(dir.filePath("table.csv"), "a,b\n1,2\n");
        preview.show(dir.filePath("table.csv"));
        QCOMPARE(preview.documentKind(), QStringLiteral("csv"));
        QTRY_VERIFY_WITH_TIMEOUT(preview.html().contains("<th>a</th>"), 10000);
        preview.show(dir.filePath("data.txt"));
        QCOMPARE(preview.kind(), QStringLiteral("other"));
        preview.show(dir.filePath("big.txt"));
        QVERIFY(preview.truncated());
        QCOMPARE(preview.text().size(), Preview::MaxText);
        // Sound and video are played, by their type.
        write(dir.filePath("song.mp3"), "ID3");
        write(dir.filePath("clip.mp4"), QByteArray("\0\0\0\x18ftypmp42", 12));
        preview.show(dir.filePath("song.mp3"));
        QCOMPARE(preview.kind(), QStringLiteral("audio"));
        preview.show(dir.filePath("clip.mp4"));
        QCOMPARE(preview.kind(), QStringLiteral("video"));
        preview.show(dir.path());
        QCOMPARE(preview.kind(), QStringLiteral("folder"));
        QCOMPARE(preview.details().value("items").toInt(), 9);
    }

    void aPdfIsRenderedPageByPage() {
        if (Sandbox::program("pdftoppm").isEmpty() || Sandbox::program("pdfinfo").isEmpty()) {
            QSKIP("poppler-utils is not installed");
        }
        QTemporaryDir dir;
        {
            QPdfWriter pdf(dir.filePath("two.pdf"));
            QPainter painter(&pdf);
            painter.fillRect(QRect(0, 0, 2000, 2000), Qt::red);
            pdf.newPage();
            painter.fillRect(QRect(0, 0, 2000, 2000), Qt::blue);
        }
        Preview preview;
        preview.show(dir.filePath("two.pdf"));
        QCOMPARE(preview.kind(), QStringLiteral("pdf"));
        QVERIFY(QTest::qWaitFor([&] { return !preview.loading() && !preview.image().isNull(); }, 20000));
        QCOMPARE(preview.pageCount(), 2);
        QCOMPARE(preview.image().pixelColor(50, 50), QColor(Qt::red));
        preview.goToPage(2);
        QVERIFY(QTest::qWaitFor([&] { return !preview.loading() && preview.image().pixelColor(50, 50) == QColor(Qt::blue); }, 20000));
    }
};

QTEST_MAIN(TestPreview)
#include "tst_preview.moc"
