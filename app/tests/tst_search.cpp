// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FolderModel.h"
#include "models/SearchModel.h"
#include "settings/AppSettings.h"

#include <QAbstractItemModelTester>
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>

namespace {

void touch(const QString &path) {
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
}

} // namespace

class TestSearch : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void typingSearchesOnceItPauses() {
        QTemporaryDir dir;
        QDir().mkpath(dir.filePath("work/q3"));
        touch(dir.filePath("work/q3/budget-final.ods"));
        touch(dir.filePath("budget.ods"));
        touch(dir.filePath("photo.jpg"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        SearchModel search(&backend);
        QAbstractItemModelTester tester(&search, QAbstractItemModelTester::FailureReportingMode::QtTest);
        // Typed a key at a time: only the last query runs.
        search.search("b", {dir.path()});
        search.search("bu", {dir.path()});
        search.search("budget", {dir.path()});
        QVERIFY(QTest::qWaitFor([&] { return search.running(); }, 2000));
        QTRY_VERIFY_WITH_TIMEOUT(!search.running(), 10000);
        QCOMPARE(search.rowCount(), 2);
        QCOMPARE(search.data(search.index(0), ItemListModel::NameRole).toString(), QStringLiteral("budget.ods"));
        QCOMPARE(search.pathAt(1), dir.filePath("work/q3/budget-final.ods"));
        QVERIFY(!search.truncated());
        // An empty query clears it.
        search.search("", {dir.path()});
        search.searchNow();
        QCOMPARE(search.rowCount(), 0);
    }

    void foldersLeftOutAreNotSearched() {
        QTemporaryDir dir;
        QDir().mkpath(dir.filePath("Archive"));
        QDir().mkpath(dir.filePath("live"));
        touch(dir.filePath("Archive/plan.txt"));
        touch(dir.filePath("live/plan.txt"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        SearchModel search(&backend);
        // A rule is kept as the backend keeps it; one that can't be is refused, saying why.
        QSignalSpy changed(&AppSettings::shared(), &AppSettings::changed);
        search.addExclusion(" archive/ ");
        QVERIFY(changed.wait(10000));
        QCOMPARE(AppSettings::shared().searchExclusions(), QStringList{"archive"});
        QSignalSpy refused(&search, &SearchModel::exclusionRefused);
        search.addExclusion("~");
        QVERIFY(refused.wait(10000));
        QVERIFY(!refused.first().first().toString().isEmpty());
        QCOMPARE(AppSettings::shared().searchExclusions(), QStringList{"archive"});

        search.search("plan", {dir.path()});
        search.searchNow();
        QTRY_VERIFY_WITH_TIMEOUT(!search.running() && search.rowCount() > 0, 10000);
        QCOMPARE(search.rowCount(), 1);
        QCOMPARE(search.pathAt(0), dir.filePath("live/plan.txt"));
        AppSettings::shared().setSearchExclusions({});
    }

    void aFolderIsFilteredByTyping() {
        QTemporaryDir dir;
        for (const char *name : {"report.pdf", "notes.txt", "report-old.pdf"}) {
            touch(dir.filePath(QLatin1String(name)));
        }
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.openPath(dir.path());
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.rowCount(), 3);
        model.setFilter("rep");
        QTRY_COMPARE(model.rowCount(), 2);
        model.setFilter("rep !old");
        QTRY_COMPARE(model.rowCount(), 1);
        model.setFilter("");
        QTRY_COMPARE(model.rowCount(), 3);
        // Another folder opens unfiltered.
        model.setFilter("notes");
        QTRY_COMPARE(model.rowCount(), 1);
        model.openPath(dir.path() + "/..");
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.filter(), QString());
    }
};

QTEST_GUILESS_MAIN(TestSearch)
#include "tst_search.moc"
