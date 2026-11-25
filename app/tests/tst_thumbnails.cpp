// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "render/Sandbox.h"
#include "thumbnails/Thumbnails.h"

#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QImageReader>
#include <QPainter>
#include <QPdfWriter>
#include <QProcess>
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

    void videosAndPdfsHaveThumbnailsToo() {
        QVERIFY(Thumbnails::canMake("clip.MP4"));
        QVERIFY(Thumbnails::canMake("paper.pdf"));
        QVERIFY(!Thumbnails::canMake("notes.txt"));
        QTemporaryDir dir;
        // A PDF: its first page.
        if (!Sandbox::program("pdftoppm").isEmpty()) {
            {
                QPdfWriter pdf(dir.filePath("paper.pdf"));
                QPainter painter(&pdf);
                painter.fillRect(QRect(0, 0, 2000, 2000), Qt::blue);
            }
            const QImage page = Thumbnails::thumbnail(QFile::encodeName(dir.filePath("paper.pdf")), 128);
            QVERIFY(!page.isNull());
            QVERIFY(qMax(page.width(), page.height()) <= 128);
        } else {
            qInfo("pdftoppm is not installed: no PDF thumbnail");
        }
        // A video: a frame of it.
        const QString ffmpeg = QStandardPaths::findExecutable("ffmpeg");
        if (ffmpeg.isEmpty()) {
            QSKIP("ffmpeg is not installed to make a video");
        }
        QProcess make;
        make.start(ffmpeg, {"-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=green:size=320x180:rate=10", "-t", "2",
                            "-c:v", "mjpeg", dir.filePath("clip.avi")});
        QVERIFY(make.waitForFinished(30000) && make.exitCode() == 0);
        const QImage frame = Thumbnails::thumbnail(QFile::encodeName(dir.filePath("clip.avi")), 128);
        QVERIFY(!frame.isNull());
        QCOMPARE(frame.size(), QSize(128, 72));
        const QColor middle = frame.pixelColor(64, 36);
        QVERIFY2(middle.green() > 100 && middle.red() < 60, qPrintable(middle.name()));

        // Sound with a cover: the cover. Without one: none.
        QImage red(200, 200, QImage::Format_RGB32);
        red.fill(Qt::red);
        QVERIFY(red.save(dir.filePath("cover.png")));
        make.start(ffmpeg, {"-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-i", dir.filePath("cover.png"),
                            "-map", "0:a", "-map", "1:v", "-c:a", "flac", "-c:v", "png", "-disposition:v", "attached_pic", dir.filePath("song.flac")});
        QVERIFY(make.waitForFinished(30000) && make.exitCode() == 0);
        const QImage cover = Thumbnails::thumbnail(QFile::encodeName(dir.filePath("song.flac")), 128);
        QVERIFY(!cover.isNull());
        QCOMPARE(cover.size(), QSize(128, 128));
        QVERIFY2(cover.pixelColor(64, 64).red() > 200, qPrintable(cover.pixelColor(64, 64).name()));
        make.start(ffmpeg, {"-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-c:a", "flac", dir.filePath("plain.flac")});
        QVERIFY(make.waitForFinished(30000) && make.exitCode() == 0);
        QVERIFY(Thumbnails::thumbnail(QFile::encodeName(dir.filePath("plain.flac")), 128).isNull());
    }

    void comicsBooksAndModelsShowTheirCovers() {
        QVERIFY(Thumbnails::canMake("issue.CBZ"));
        QVERIFY(Thumbnails::canMake("novel.epub"));
        QVERIFY(Thumbnails::canMake("part.3mf"));
        QVERIFY(Thumbnails::canMake("part.FCStd"));
        const QString python = QStandardPaths::findExecutable("python3");
        if (python.isEmpty()) {
            QSKIP("python3 is not installed to make an archive");
        }
        QTemporaryDir dir;
        QImage page(200, 300, QImage::Format_RGB32);
        page.fill(Qt::red);
        QVERIFY(page.save(dir.filePath("page.png")));
        // A comic whose pages are out of order in the archive: the first page is the cover.
        QImage other(200, 300, QImage::Format_RGB32);
        other.fill(Qt::blue);
        QVERIFY(other.save(dir.filePath("other.png")));
        QProcess make;
        make.start(python, {"-I", "-c", "import sys, zipfile\n"
                                        "z = zipfile.ZipFile(sys.argv[1], 'w')\n"
                                        "z.write(sys.argv[3], 'page10.png')\n"
                                        "z.write(sys.argv[2], 'page2.png')\n"
                                        "z.close()",
                            dir.filePath("issue.cbz"), dir.filePath("page.png"), dir.filePath("other.png")});
        QVERIFY(make.waitForFinished(30000) && make.exitCode() == 0);
        const QImage cover = Thumbnails::thumbnail(QFile::encodeName(dir.filePath("issue.cbz")), 128);
        QVERIFY(!cover.isNull());
        QVERIFY(qMax(cover.width(), cover.height()) <= 256);
        QVERIFY2(cover.pixelColor(cover.width() / 2, cover.height() / 2).red() > 200, "the first page, by its number");
        // One without a picture: none.
        make.start(python, {"-I", "-c", "import sys, zipfile\nz = zipfile.ZipFile(sys.argv[1], 'w')\nz.writestr('notes.txt', 'x')\nz.close()",
                            dir.filePath("empty.cbz")});
        QVERIFY(make.waitForFinished(30000) && make.exitCode() == 0);
        QVERIFY(Thumbnails::thumbnail(QFile::encodeName(dir.filePath("empty.cbz")), 128).isNull());
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

QTEST_MAIN(TestThumbnails)
#include "tst_thumbnails.moc"
