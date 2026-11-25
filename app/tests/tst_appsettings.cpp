// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "models/FolderViews.h"
#include "settings/AppSettings.h"

#include <QDateTime>
#include <QDir>
#include <QSignalSpy>
#include <QTemporaryDir>

class TestAppSettings : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void defaultsAreLegacys() {
        const AppSettings &settings = AppSettings::shared();
        QVERIFY(settings.restoreTabs());
        QCOMPARE(settings.dateFormat(), QStringLiteral("relative"));
        QVERIFY(!settings.windowButtons());
        QVERIFY(settings.rememberFolderViews());
        QCOMPARE(settings.previewStyle(), QStringLiteral("popup"));
        QVERIFY(settings.showThumbnails());
        // Files open with a double click everywhere; folders with one in Columns.
        QCOMPARE(settings.clicks(FolderViews::List, true), 2);
        QCOMPARE(settings.clicks(FolderViews::List, false), 2);
        QCOMPARE(settings.clicks(FolderViews::Columns, true), 2);
        QCOMPARE(settings.clicks(FolderViews::Columns, false), 1);
        QCOMPARE(settings.clickCounts().value("2/folders").toInt(), 1);
    }

    void changesAreKeptAndAnnounced() {
        AppSettings &settings = AppSettings::shared();
        QSignalSpy changed(&settings, &AppSettings::changed);
        settings.setClicks(FolderViews::Icons, false, 1);
        QCOMPARE(settings.clicks(FolderViews::Icons, false), 1);
        QCOMPARE(changed.count(), 1);
        // The same value again changes nothing.
        settings.setClicks(FolderViews::Icons, false, 1);
        QCOMPARE(changed.count(), 1);
        QSettings file(QSettings::IniFormat, QSettings::UserScope, "asc-files", "asc-files");
        QCOMPARE(file.value("clicks/1/folders").toInt(), 1);
        settings.setClicks(FolderViews::Icons, false, 2);
        // Values out of range are refused or held.
        settings.setDefaultSort(QStringLiteral("colour"));
        QCOMPARE(settings.defaultSort(), QStringLiteral("name"));
        settings.setDefaultView(7);
        QCOMPARE(settings.defaultView(), int(FolderViews::Columns));
        settings.setDefaultView(FolderViews::List);
        settings.setPreviewStyle(QStringLiteral("sideways"));
        QCOMPARE(settings.previewStyle(), QStringLiteral("popup"));
    }

    void aStartFolderGoneFallsBackToHome() {
        AppSettings &settings = AppSettings::shared();
        QCOMPARE(settings.startFolderOrHome(), QDir::homePath());
        {
            QTemporaryDir dir;
            settings.setStartFolder(dir.path());
            QCOMPARE(settings.startFolderOrHome(), dir.path());
        }
        QCOMPARE(settings.startFolderOrHome(), QDir::homePath());
        settings.setStartFolder({});
    }

    void datesReadAsChosen() {
        AppSettings &settings = AppSettings::shared();
        const QDateTime now(QDate(2026, 11, 24), QTime(14, 5));
        QSignalSpy reworded(&settings, &AppSettings::dateFormatChanged);
        QCOMPARE(settings.modifiedText(now.addSecs(-300), now), QStringLiteral("5m ago"));
        QCOMPARE(settings.modifiedText(now.addSecs(-7200), now), QStringLiteral("2h ago"));
        settings.setDateFormat(QStringLiteral("iso"));
        QCOMPARE(reworded.count(), 1);
        QCOMPARE(settings.modifiedText(now.addSecs(-300), now), QStringLiteral("2026-11-24 14:00"));
        settings.setDateFormat(QStringLiteral("long"));
        QVERIFY(settings.modifiedText(now, now).contains(QStringLiteral("2026")));
        settings.setDateFormat(QStringLiteral("binary"));
        QCOMPARE(settings.dateFormat(), QStringLiteral("long"));
        settings.setDateFormat(QStringLiteral("relative"));
        QCOMPARE(settings.dateExample(QStringLiteral("relative")), QStringLiteral("5m ago"));
    }
};

QTEST_GUILESS_MAIN(TestAppSettings)
#include "tst_appsettings.moc"
