// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/Browser.h"
#include "models/FolderModel.h"
#include "models/TabModel.h"

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
};

QTEST_GUILESS_MAIN(TestBrowser)
#include "tst_browser.moc"
