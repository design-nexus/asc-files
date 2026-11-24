// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/Browser.h"
#include "models/FavoritesModel.h"
#include "models/RecentModel.h"
#include "models/TrashModel.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/PlacesModel.h"
#include "models/TabModel.h"

#include <QFile>
#include <QQmlApplicationEngine>
#include <QQmlExtensionPlugin>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTemporaryDir>

Q_IMPORT_QML_PLUGIN(AscFilesPlugin)

class TestWindow : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        QQuickStyle::setStyle(QStringLiteral("Basic"));
    }

    void theWindowShowsAFolderWithoutWarnings() {
        QTest::failOnWarning(QRegularExpression(QStringLiteral(".*")));
        QTemporaryDir dir;
        for (const char *name : {"b.txt", "a.md"}) {
            QFile file(dir.filePath(QString::fromLatin1(name)));
            QVERIFY(file.open(QIODevice::WriteOnly));
        }
        QDir().mkdir(dir.filePath("folder"));

        BackendClient backend(TestSupport::backend());
        backend.start();
        TabModel tabs(&backend);
        PlacesModel places;
        RecentModel recent;
        FavoritesModel favorites;
        TrashModel trash(&backend);
        FolderModel downloads(&backend);
        JobModel jobs(&backend);
        tabs.openTab(dir.path());
        FolderModel &folders = *tabs.current()->folderModel();
        QSignalSpy opened(&folders, &FolderModel::opened);
        QQmlApplicationEngine engine;
        engine.setInitialProperties({{"tabs", QVariant::fromValue(&tabs)},
                                     {"places", QVariant::fromValue(&places)},
                                     {"recent", QVariant::fromValue(&recent)},
                                     {"favorites", QVariant::fromValue(&favorites)},
                                     {"trash", QVariant::fromValue(&trash)},
                                     {"downloads", QVariant::fromValue(&downloads)},
                                     {"jobModel", QVariant::fromValue(&jobs)},
                                     {"backend", QVariant::fromValue(&backend)}});
        engine.loadFromModule("AscFiles", "Main");
        QCOMPARE(engine.rootObjects().size(), 1);
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        QVERIFY(window);
        QVERIFY(opened.count() > 0 || opened.wait(10000));
        auto *list = window->findChild<QQuickItem *>(QStringLiteral("fileList"));
        QVERIFY(list);
        QTRY_COMPARE(list->property("count").toInt(), 3);
        auto *sidebar = window->findChild<QQuickItem *>(QStringLiteral("places"));
        QVERIFY(sidebar);
        QVERIFY(sidebar->property("count").toInt() >= 2);

        // A second tab, then back to the first.
        tabs.openTab(QDir::homePath());
        QCOMPARE(tabs.rowCount(), 2);
        QTRY_VERIFY(tabs.current()->folderModel()->rowCount() >= 0);
        tabs.setCurrentIndex(0);
        QTRY_COMPARE(list->property("count").toInt(), 3);
        QTRY_VERIFY(folders.isLoaded(0));
        QVERIFY(QTest::qWaitForWindowExposed(window));

        // Every page shows without a warning.
        for (const Browser::Page page : {Browser::Overview, Browser::Recent, Browser::Favorites, Browser::Trash}) {
            tabs.current()->goPage(page);
            QTest::qWait(100);
        }
        tabs.current()->back();
        tabs.current()->back();
        tabs.current()->back();
        tabs.current()->back();
        QTRY_COMPARE(tabs.current()->page(), Browser::Folder);

        // Tiled narrow, the panel and its list stay inside the window.
        window->resize(800, 900);
        QTRY_COMPARE(window->width(), 800);
        const auto rightEdge = [&] { return list->mapToScene(QPointF(list->width(), 0)).x(); };
        QTRY_VERIFY2(rightEdge() <= window->width() && list->width() > 200, qPrintable(QString::number(rightEdge())));

        // The sidebar as an icon rail, then hidden, then full again: the list widens each time.
        window->resize(1400, 900);
        QTRY_COMPARE(sidebar->width(), 229.0);
        const qreal full = list->width();
        places.stepMode();
        QTRY_COMPARE(sidebar->width(), 56.0);
        QTRY_VERIFY(list->width() > full);
        places.stepMode();
        QTRY_VERIFY(!sidebar->isVisible());
        places.stepMode();
        QTRY_VERIFY(sidebar->isVisible());
        QTRY_COMPARE(sidebar->width(), 229.0);
    }
};

QTEST_MAIN(TestWindow)
#include "tst_window.moc"
