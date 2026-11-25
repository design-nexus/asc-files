// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/Browser.h"
#include "models/FolderModel.h"
#include "models/ColumnsModel.h"
#include "models/FolderViews.h"
#include "models/TabModel.h"
#include "settings/AppSettings.h"

#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QVariantMap>

namespace {

bool openedAt(Browser &browser, const QString &path) {
    return QTest::qWaitFor([&] { return browser.location() == path && !browser.folderModel()->loading(); }, 10000);
}

QStringList labels(const Browser &browser) {
    QStringList result;
    for (const QVariant &crumb : browser.crumbs()) {
        result.append(crumb.toMap().value("label").toString());
    }
    return result;
}

} // namespace

class TestBrowser : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void historyGoesBackAndForward() {
        QTemporaryDir dir;
        QVERIFY(QDir().mkpath(dir.filePath("a/b")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.path());
        QVERIFY(openedAt(browser, dir.path()));
        QVERIFY(!browser.canGoBack());
        browser.goPath(dir.filePath("a"));
        QVERIFY(openedAt(browser, dir.filePath("a")));
        browser.goPath(dir.filePath("a/b"));
        QVERIFY(openedAt(browser, dir.filePath("a/b")));
        browser.back();
        QVERIFY(openedAt(browser, dir.filePath("a")));
        QVERIFY(browser.canGoForward());
        browser.back();
        QVERIFY(openedAt(browser, dir.path()));
        QVERIFY(!browser.canGoBack());
        browser.forward();
        QVERIFY(openedAt(browser, dir.filePath("a")));
        // Going somewhere new drops what was ahead.
        browser.up();
        QVERIFY(openedAt(browser, dir.path()));
        QVERIFY(!browser.canGoForward());
    }

    void crumbsStartAtHome() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("crumbs-XXXXXX")));
        QVERIFY(QDir().mkpath(dir.filePath("inner")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.filePath("inner"));
        QVERIFY(openedAt(browser, dir.filePath("inner")));
        QCOMPARE(labels(browser), (QStringList{"Home", QFileInfo(dir.path()).fileName(), "inner"}));
        QVERIFY(browser.crumbs().first().toMap().value("home").toBool());
        browser.goCrumb(0);
        QVERIFY(openedAt(browser, QDir::homePath()));
        QCOMPARE(browser.title(), QStringLiteral("Home"));

        browser.goPath(QStringLiteral("/"));
        QVERIFY(openedAt(browser, QStringLiteral("/")));
        QCOMPARE(labels(browser), QStringList{"Computer"});
        QVERIFY(!browser.canGoUp());
    }

    void typedPathsGoWhereTheyName() {
        QTemporaryDir dir;
        QVERIFY(QDir().mkpath(dir.filePath("x/y")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.path());
        QVERIFY(openedAt(browser, dir.path()));
        QVERIFY(browser.goTyped(QStringLiteral("x/y")));
        QVERIFY(openedAt(browser, dir.filePath("x/y")));
        QVERIFY(browser.goTyped(QStringLiteral("../..")));
        QVERIFY(openedAt(browser, dir.path()));
        QVERIFY(browser.goTyped(QStringLiteral("~")));
        QVERIFY(openedAt(browser, QDir::homePath()));
        QVERIFY(!browser.goTyped(QStringLiteral("   ")));
    }

    void pagesTakeTheirPlaceInHistory() {
        QTemporaryDir dir;
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.path());
        QVERIFY(openedAt(browser, dir.path()));
        browser.goPage(Browser::Trash);
        QCOMPARE(browser.page(), Browser::Trash);
        QCOMPARE(browser.title(), QStringLiteral("Trash"));
        QCOMPARE(browser.location(), QString());
        QCOMPARE(labels(browser), QStringList{"Trash"});
        QVERIFY(!browser.canGoUp());
        QCOMPARE(browser.saved(), QStringLiteral("page:trash"));
        browser.back();
        QCOMPARE(browser.page(), Browser::Folder);
        QVERIFY(openedAt(browser, dir.path()));
        browser.forward();
        QCOMPARE(browser.page(), Browser::Trash);
        // A relative path typed on a page is taken from Home.
        QVERIFY(browser.goTyped(QStringLiteral(".")));
        QVERIFY(openedAt(browser, QDir::homePath()));
    }

    void aSessionsTabsComeBack() {
        QTemporaryDir dir;
        {
            BackendClient backend(TestSupport::backend());
            backend.start();
            TabModel tabs(&backend);
            tabs.openTab(dir.path());
            tabs.openTab(QDir::homePath());
            tabs.current()->goPage(Browser::Recent);
            tabs.setCurrentIndex(0);
            // Saved a moment after the last change.
            QTest::qWait(800);
        }
        BackendClient backend(TestSupport::backend());
        backend.start();
        TabModel tabs(&backend);
        QVERIFY(tabs.restoreSession());
        QCOMPARE(tabs.rowCount(), 2);
        QCOMPARE(tabs.currentIndex(), 0);
        QVERIFY(openedAt(*tabs.at(0), dir.path()));
        QCOMPARE(tabs.at(1)->page(), Browser::Recent);
        // A new tab opened on a page starts on that page.
        tabs.setCurrentIndex(1);
        tabs.openTab();
        QCOMPARE(tabs.current()->page(), Browser::Recent);
    }

    void eachFolderKeepsItsViewAndSort() {
        QTemporaryDir dir;
        QVERIFY(QDir().mkpath(dir.filePath("photos")));
        QVERIFY(QDir().mkpath(dir.filePath("papers")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.filePath("photos"));
        QVERIFY(openedAt(browser, dir.filePath("photos")));
        QCOMPARE(browser.view(), AppSettings::shared().defaultView());
        browser.setView(FolderViews::Icons);
        browser.sortBy(QStringLiteral("modified"));
        QCOMPARE(browser.folderModel()->sortKey(), QStringLiteral("modified"));
        QVERIFY(browser.folderModel()->sortDescending());
        // A folder never set keeps the view the tab is in, and takes the default sort.
        browser.goPath(dir.filePath("papers"));
        QVERIFY(openedAt(browser, dir.filePath("papers")));
        QCOMPARE(browser.view(), int(FolderViews::Icons));
        QCOMPARE(browser.folderModel()->sortKey(), QStringLiteral("name"));
        browser.setView(FolderViews::List);
        browser.back();
        QVERIFY(openedAt(browser, dir.filePath("photos")));
        QCOMPARE(browser.view(), int(FolderViews::Icons));
        QCOMPARE(browser.folderModel()->sortKey(), QStringLiteral("modified"));
        // Sorting by the same key again turns it the other way.
        browser.sortBy(QStringLiteral("modified"));
        QVERIFY(!browser.folderModel()->sortDescending());
        QFile saved(FolderViews::filePath());
        QVERIFY(saved.open(QIODevice::ReadOnly));
        QVERIFY(saved.readAll().contains("photos"));
    }

    void theDefaultsAndForgettingComeFromSettings() {
        QTemporaryDir dir;
        QVERIFY(QDir().mkpath(dir.filePath("a")));
        QVERIFY(QDir().mkpath(dir.filePath("b")));
        AppSettings &settings = AppSettings::shared();
        settings.setDefaultView(FolderViews::Columns);
        settings.setDefaultSort(QStringLiteral("size"));
        settings.setDefaultDescending(true);
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.filePath("a"));
        QVERIFY(openedAt(browser, dir.filePath("a")));
        QCOMPARE(browser.view(), int(FolderViews::Columns));
        QCOMPARE(browser.folderModel()->sortKey(), QStringLiteral("size"));
        QVERIFY(browser.folderModel()->sortDescending());

        // Not kept per folder: the tab's view goes along, and nothing is written.
        settings.setRememberFolderViews(false);
        browser.setView(FolderViews::Icons);
        QVERIFY(!FolderViews::shared().has(dir.filePath("a")));
        browser.goPath(dir.filePath("b"));
        QVERIFY(openedAt(browser, dir.filePath("b")));
        QCOMPARE(browser.view(), int(FolderViews::Icons));

        settings.setRememberFolderViews(true);
        browser.setView(FolderViews::List);
        QVERIFY(settings.hasFolderViews());
        settings.forgetFolderViews();
        QVERIFY(!settings.hasFolderViews());
        settings.setDefaultView(FolderViews::List);
        settings.setDefaultSort(QStringLiteral("name"));
        settings.setDefaultDescending(false);
    }

    void columnsRunFromHomeToTheFolder() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("columns-XXXXXX")));
        QVERIFY(QDir().mkpath(dir.filePath("a/b")));
        QVERIFY(QDir().mkpath(dir.filePath("a/c")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        Browser browser(&backend);
        browser.goPath(dir.filePath("a/b"));
        QVERIFY(openedAt(browser, dir.filePath("a/b")));
        browser.setView(FolderViews::Columns);
        ColumnsModel *columns = browser.columns();
        QCOMPARE(columns->rowCount(), 4);
        QCOMPARE(columns->data(columns->index(0), ColumnsModel::OpenNameRole).toString(), QFileInfo(dir.path()).fileName());
        QCOMPARE(columns->data(columns->index(2), ColumnsModel::OpenNameRole).toString(), QStringLiteral("b"));
        // The last column is the tab's own listing.
        QCOMPARE(columns->column(3), browser.folderModel());
        FolderModel *kept = columns->column(1);
        // Across to a sibling: the shared columns stay.
        browser.goPath(dir.filePath("a/c"));
        QVERIFY(openedAt(browser, dir.filePath("a/c")));
        QCOMPARE(columns->rowCount(), 4);
        QCOMPARE(columns->column(1), kept);
        QCOMPARE(columns->data(columns->index(2), ColumnsModel::OpenNameRole).toString(), QStringLiteral("c"));
        // Up: one column fewer.
        browser.up();
        QVERIFY(openedAt(browser, dir.filePath("a")));
        QCOMPARE(columns->rowCount(), 3);
        browser.setView(FolderViews::List);
    }

    void tabsOpenCloseAndMove() {
        QTemporaryDir dir;
        BackendClient backend(TestSupport::backend());
        backend.start();
        TabModel tabs(&backend);
        tabs.openTab(dir.path());
        tabs.openTab(QDir::homePath());
        QCOMPARE(tabs.rowCount(), 2);
        QCOMPARE(tabs.currentIndex(), 1);
        QVERIFY(openedAt(*tabs.at(0), dir.path()));
        // A new tab without a path opens on the current folder.
        tabs.openTab();
        QCOMPARE(tabs.rowCount(), 3);
        QVERIFY(openedAt(*tabs.current(), QDir::homePath()));
        tabs.moveTab(2, 0);
        QCOMPARE(tabs.currentIndex(), 0);
        tabs.closeTab(0);
        QCOMPARE(tabs.rowCount(), 2);
        tabs.closeTab(0);
        tabs.closeTab(0);
        QCOMPARE(tabs.rowCount(), 1);
        tabs.next();
        QCOMPARE(tabs.currentIndex(), 0);
    }

    void tabsAreDuplicatedAndClosedTogether() {
        QTemporaryDir dir;
        BackendClient backend(TestSupport::backend());
        backend.start();
        TabModel tabs(&backend);
        tabs.openTab(dir.path());
        tabs.openTab(QDir::homePath());
        tabs.current()->goPage(Browser::Trash);
        // A duplicate sits beside its tab, on the same folder or page.
        tabs.duplicateTab(0);
        QCOMPARE(tabs.rowCount(), 3);
        QCOMPARE(tabs.currentIndex(), 1);
        QVERIFY(openedAt(*tabs.at(1), dir.path()));
        QCOMPARE(tabs.folderAt(1), dir.path());
        tabs.duplicateTab(2);
        QCOMPARE(tabs.at(3)->page(), Browser::Trash);
        QCOMPARE(tabs.folderAt(3), QString());
        // Those after a tab, then all others.
        tabs.closeTabsAfter(1);
        QCOMPARE(tabs.rowCount(), 2);
        tabs.closeOtherTabs(1);
        QCOMPARE(tabs.rowCount(), 1);
        QCOMPARE(tabs.currentIndex(), 0);
        QCOMPARE(tabs.folderAt(0), dir.path());
    }
};

QTEST_GUILESS_MAIN(TestBrowser)
#include "tst_browser.moc"
