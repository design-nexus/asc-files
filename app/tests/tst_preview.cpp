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
        preview.show(dir.filePath("notes.md"));
        QCOMPARE(preview.kind(), QStringLiteral("markdown"));
        QVERIFY(preview.text().startsWith("# Title"));
        preview.show(dir.filePath("data.txt"));
        QCOMPARE(preview.kind(), QStringLiteral("other"));
        preview.show(dir.filePath("big.txt"));
        QVERIFY(preview.truncated());
        QCOMPARE(preview.text().size(), Preview::MaxText);
        preview.show(dir.path());
        QCOMPARE(preview.kind(), QStringLiteral("folder"));
        QCOMPARE(preview.details().value("items").toInt(), 4);
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
