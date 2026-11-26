// SPDX-License-Identifier: MIT
//! Search's terms, the folders opened (z / Z) and 10xer's folder picker.
#include "history/FolderPicker.h"
#include "history/Terms.h"
#include "history/Visits.h"

#include <QDir>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

class TestHistory : public QObject {
    Q_OBJECT
private slots:
    void termsReadAsSearchReadsThem() {
        QVERIFY(Terms("git trading readme").matches("/home/u/git/trading/README.md"));
        QVERIFY(Terms("dev str").matches("/home/u/dev/strata"));
        QVERIFY(!Terms("dev str").matches("/home/u/dev/notes"));
        QVERIFY(Terms("'trad").matches("/x/trading"));
        QVERIFY(!Terms("'tdg").matches("/x/trading"));
        QVERIFY(Terms("^/home").matches("/home/u"));
        QVERIFY(!Terms("^u").matches("/home/u"));
        QVERIFY(Terms("md$").matches("/a/b.md"));
        QVERIFY(!Terms("!cache").matches("/home/u/.cache/x"));
        QVERIFY(Terms("").matches("/anything"));
        QCOMPARE(Terms("rep md").inName("gamma-report.md"), 2);
        QCOMPARE(Terms("dev str !x").inName("strata"), 1);
        // A run of the letters is closer than letters spread through the name.
        QCOMPARE(Terms("proj").looseness("Projects"), 0);
        QVERIFY(Terms("proj").looseness("eslint-plugin-spidermonkey-js") > Terms("proj").looseness("pro-jects"));
    }

    void zRanksNamesThenFrecencyAndZTheLatest() {
        Visits visits(QString{});
        visits.record("/home/u/dev/strata");
        visits.record("/home/u/notes/strategy");
        visits.record("/home/u/notes/strategy");
        visits.record("/home/u/str");
        visits.record("/home/u/dev");
        // "str": every one but /home/u/dev; the name typed exactly first among names that match,
        // then the most visited.
        QCOMPARE(visits.jump("str"), (QStringList{"/home/u/str", "/home/u/notes/strategy", "/home/u/dev/strata"}));
        QCOMPARE(visits.jump("dev str"), (QStringList{"/home/u/dev/strata"}));
        // The folder open is left out; Z is the latest first.
        QCOMPARE(visits.recent("", "/home/u/dev").first(), QStringLiteral("/home/u/str"));
        QVERIFY(!visits.recent("", "/home/u/dev").contains("/home/u/dev"));
    }

    void visitsAreKeptBetweenSessions() {
        QTemporaryDir dir;
        const QString file = dir.filePath("state/visits.json");
        {
            Visits visits(file);
            visits.record("/a");
            visits.record("/a");
        }
        Visits again(file);
        QVERIFY(again.frecency("/a") > 0);
        QCOMPARE(again.jump("a"), QStringList{"/a"});
    }

    void theTextSaysWhereThePickerSearches() {
        const QString home = QDir::homePath();
        auto where = FolderPicker::where("proj", "/srv/open");
        QCOMPARE(where.base, QStringLiteral("/srv/open"));
        QCOMPARE(where.query, QStringLiteral("proj"));
        where = FolderPicker::where("/etc/ss", "/srv");
        QCOMPARE(where.base, QStringLiteral("/etc"));
        QCOMPARE(where.query, QStringLiteral("ss"));
        where = FolderPicker::where("~/dev/ str", "/srv");
        QCOMPARE(where.base, home + "/dev");
        QCOMPARE(where.query, QStringLiteral(" str"));
        where = FolderPicker::where("../", "/srv/open");
        QCOMPARE(where.base, QStringLiteral("/srv"));
        QVERIFY(where.itself);
        where = FolderPicker::where("..", "/srv/open");
        QCOMPARE(where.base, QStringLiteral("/srv"));
        QVERIFY(!FolderPicker::where("~root/x", "/srv").error.isEmpty());
        QVERIFY(!FolderPicker::where("proj", "").error.isEmpty());
        QVERIFY(FolderPicker::where("/etc/", "").error.isEmpty());
        QCOMPARE(FolderPicker::where("10:30", "/srv").query, QStringLiteral("10:30"));
    }

    void thePickerListsMatchingFoldersBestFirst() {
        QTemporaryDir dir;
        QDir root(dir.path());
        for (const char *path : {"code/strata", "notes/strategy", "notes/strata-notes", "strata", ".hidden/strata", "code/other"}) {
            QVERIFY(root.mkpath(path));
        }
        QFile file(root.filePath("code/strata-file"));
        QVERIFY(file.open(QIODevice::WriteOnly));
        const std::atomic_bool cancelled = false;
        QHash<QString, double> frecencies{{root.filePath("notes/strata-notes"), 8}};
        QStringList found = FolderPicker::find(FolderPicker::where("strata", dir.path()), false, frecencies, cancelled);
        // The folder the query names outright, then by name (the visited first, then shallow);
        // files and hidden folders are not listed.
        QCOMPARE(found.first(), root.filePath("strata"));
        QVERIFY(found.contains(root.filePath("code/strata")));
        QVERIFY(found.indexOf(root.filePath("notes/strata-notes")) < found.indexOf(root.filePath("code/strata")) ||
                found.indexOf(root.filePath("code/strata")) == 1);
        QVERIFY(!found.contains(root.filePath("code/strata-file")));
        QVERIFY(!found.contains(root.filePath(".hidden/strata")));
        QVERIFY(!found.contains(root.filePath("code/other")));
        // Hidden ones when asked, or a term starting with a dot.
        QVERIFY(FolderPicker::find(FolderPicker::where("strata", dir.path()), true, {}, cancelled).contains(root.filePath(".hidden/strata")));
        QVERIFY(FolderPicker::find(FolderPicker::where(".hid", dir.path()), false, {}, cancelled).contains(root.filePath(".hidden")));
        // A path ending in "/": that folder first, then what is below it.
        found = FolderPicker::find(FolderPicker::where(dir.path() + "/notes/", ""), false, {}, cancelled);
        QCOMPARE(found.first(), root.filePath("notes"));
        QCOMPARE(found.size(), 3);
    }

    void thePickerAnswersOffTheUiThread() {
        QTemporaryDir dir;
        QVERIFY(QDir(dir.path()).mkpath("a/b/target"));
        FolderPicker picker;
        QSignalSpy changed(&picker, &FolderPicker::changed);
        picker.search("targ", dir.path(), false);
        QVERIFY(picker.searching());
        QTRY_VERIFY(!picker.searching());
        QCOMPARE(picker.folders(), QStringList{dir.filePath("a/b/target")});
        picker.search("nothing-like-this", dir.path(), false);
        QTRY_VERIFY(!picker.searching());
        QCOMPARE(picker.message(), QStringLiteral("No matching folders"));
        picker.search("~nobody/x", dir.path(), false);
        QCOMPARE(picker.message(), QStringLiteral("Only ~ and ~/ are supported"));
    }
};

QTEST_GUILESS_MAIN(TestHistory)
#include "tst_history.moc"
