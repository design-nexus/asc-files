// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "thumbnails/Thumbnails.h"

#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QUrl>
#include <sys/stat.h>
#include <utime.h>

class TestThumbnails : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        QVERIFY(QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation).contains("asc-files-test-home"));
    }

    void aThumbnailIsMadeKeptAndRemade() {
        QTemporaryDir dir;
        const QString picture = dir.filePath("wide.png");
        QImage image(800, 400, QImage::Format_RGB32);
        image.fill(Qt::red);
        QVERIFY(image.save(picture));
        const QByteArray path = QFile::encodeName(picture);

        const QImage first = Thumbnails::thumbnail(path, 100);
        QCOMPARE(first.size(), QSize(128, 64));
        // Kept by the standard's name, with the file's URI and time.
        const QString kept = Thumbnails::cachePath(path, 128);
        QVERIFY(kept.contains("/thumbnails/normal/"));
        const QImage stored(kept);
        QCOMPARE(stored.text("Thumb::URI"), QUrl::fromLocalFile(picture).toString(QUrl::FullyEncoded));
        QVERIFY(!stored.text("Thumb::MTime").isEmpty());
        // The second time it comes from the cache: the kept file is not written again.
        const QDateTime written = QFileInfo(kept).lastModified();
        QTest::qWait(20);
        QCOMPARE(Thumbnails::thumbnail(path, 100).size(), QSize(128, 64));
        QCOMPARE(QFileInfo(kept).lastModified(), written);
        QCOMPARE(QFile::permissions(kept) & (QFile::ReadOther | QFile::ReadGroup), QFile::Permissions());

        // A larger tile takes the large size.
        QCOMPARE(Thumbnails::thumbnail(path, 200).size(), QSize(256, 128));

        // A changed file gets a new one.
        image.fill(Qt::blue);
        QVERIFY(image.save(picture));
        struct utimbuf later {QDateTime::currentSecsSinceEpoch() + 10, QDateTime::currentSecsSinceEpoch() + 10};
        QCOMPARE(::utime(path.constData(), &later), 0);
        QCOMPARE(Thumbnails::thumbnail(path, 100).pixelColor(10, 10), QColor(Qt::blue));
    }

    void whatCannotBeReadIsNotTriedAgain() {
        QTemporaryDir dir;
        const QString broken = dir.filePath("broken.jpg");
        QFile file(broken);
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.write("not a picture");
        file.close();
        QVERIFY(Thumbnails::canMake(broken));
        QVERIFY(Thumbnails::thumbnail(QFile::encodeName(broken), 100).isNull());
        // Noted as failed, by the standard's name.
        const QString hash = QFileInfo(Thumbnails::cachePath(QFile::encodeName(broken), 128)).fileName();
        QVERIFY(QFile::exists(QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation) + "/thumbnails/fail/asc-files/" + hash));
        QVERIFY(Thumbnails::thumbnail(QFile::encodeName(broken), 100).isNull());
        QVERIFY(!QFile::exists(Thumbnails::cachePath(QFile::encodeName(broken), 128)));
    }

    void onlyRegularFilesAreRead() {
        QTemporaryDir dir;
        QImage image(10, 10, QImage::Format_RGB32);
        image.fill(Qt::green);
        QVERIFY(image.save(dir.filePath("real.png")));
        QVERIFY(QFile::link(dir.filePath("real.png"), dir.filePath("link.png")));
        QVERIFY(Thumbnails::thumbnail(QFile::encodeName(dir.filePath("link.png")), 100).isNull());
        QVERIFY(!Thumbnails::canMake(QStringLiteral("notes.txt")));
        QVERIFY(Thumbnails::canMake(QStringLiteral("photo.JPEG")));
    }
};

QTEST_GUILESS_MAIN(TestThumbnails)
#include "tst_thumbnails.moc"
