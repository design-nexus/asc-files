// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "desktop/DesktopApps.h"
#include "models/Properties.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QSignalSpy>
#include <QTemporaryDir>

class TestProperties : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void aFileShowsWhatItIsAndWhoMayUseIt() {
        QTemporaryDir dir;
        QImage picture(40, 30, QImage::Format_RGB32);
        picture.fill(Qt::red);
        QVERIFY(picture.save(dir.filePath("red.png")));
        QFile::setPermissions(dir.filePath("red.png"), QFile::ReadOwner | QFile::WriteOwner | QFile::ReadGroup | QFile::ReadOther);
        BackendClient backend(TestSupport::backend());
        backend.start();
        DesktopApps apps;
        Properties properties(&backend, &apps);
        properties.setPath(dir.filePath("red.png"));
        QCOMPARE(properties.name(), QStringLiteral("red.png"));
        QCOMPARE(properties.location(), dir.path());
        QVERIFY(!properties.isFolder());
        QVERIFY(properties.sizeText().contains(QLocale().toString(QFileInfo(dir.filePath("red.png")).size())));
        QCOMPARE(properties.dimensions(), QStringLiteral("40 × 30 pixels"));
        QCOMPARE(properties.bits(), 0644);
        QCOMPARE(properties.modeText(), QStringLiteral("rw-r--r-- (644)"));
        QVERIFY(properties.canChange());
        QVERIFY(!properties.hidden());

        // Changed by the backend; the switches follow.
        QSignalSpy changed(&properties, &Properties::changed);
        properties.setBit(0020, true);
        QVERIFY(changed.wait(10000));
        QCOMPARE(properties.bits(), 0664);
        QCOMPARE(QFileInfo(dir.filePath("red.png")).permissions() & QFile::WriteGroup, QFile::WriteGroup);
        properties.setRunnable(true);
        QVERIFY(changed.wait(10000));
        QCOMPARE(properties.bits(), 0775);
        properties.setRunnable(false);
        QVERIFY(changed.wait(10000));
        QCOMPARE(properties.bits(), 0664);
        QCOMPARE(Properties::symbolic(0750), QStringLiteral("rwxr-x---"));
    }

    void aFolderIsMeasuredAsYouWatch() {
        QTemporaryDir dir;
        QDir().mkpath(dir.filePath("in/deeper"));
        for (const char *name : {"a", "in/b", "in/deeper/c", "in/.hidden"}) {
            QFile file(dir.filePath(QLatin1String(name)));
            QVERIFY(file.open(QIODevice::WriteOnly));
            file.write("1234");
        }
        BackendClient backend(TestSupport::backend());
        backend.start();
        DesktopApps apps;
        Properties properties(&backend, &apps);
        properties.setPath(dir.path());
        QVERIFY(properties.isFolder());
        QVERIFY(properties.measuring());
        QCOMPARE(properties.sizeText(), QStringLiteral("Calculating…"));
        QTRY_VERIFY_WITH_TIMEOUT(!properties.measuring(), 10000);
        QCOMPARE(properties.sizeText(), QLocale().formattedDataSize(16, 1, QLocale::DataSizeSIFormat));
        QVERIFY2(properties.containsText().startsWith(QStringLiteral("3 files")), qPrintable(properties.containsText()));
        QVERIFY(properties.measureWarning().isEmpty());
        // Another item meanwhile: the old measurement is dropped.
        properties.setPath(dir.filePath("in"));
        properties.setPath(dir.filePath("in/deeper"));
        QTRY_VERIFY_WITH_TIMEOUT(!properties.measuring(), 10000);
        QVERIFY2(properties.containsText().startsWith(QStringLiteral("1 files")), qPrintable(properties.containsText()));
        QCOMPARE(properties.opensWith(), QString());
    }
};

QTEST_GUILESS_MAIN(TestProperties)
#include "tst_properties.moc"
