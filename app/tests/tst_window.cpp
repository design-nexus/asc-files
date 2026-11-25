// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "app/Windows.h"
#include "backend/BackendClient.h"
#include "desktop/AppIcons.h"
#include "desktop/DesktopApps.h"
#include "models/ActionsModel.h"
#include "models/Browser.h"
#include "models/FavoritesModel.h"
#include "models/RecentModel.h"
#include "models/SearchModel.h"
#include "models/TrashModel.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/PlacesModel.h"
#include "models/Properties.h"
#include "models/TabModel.h"
#include "models/Transfers.h"
#include "ops/Operations.h"
#include "render/Preview.h"
#include "render/Sandbox.h"
#include "settings/AppSettings.h"
#include "theme/SystemTheme.h"
#include "thumbnails/Thumbnails.h"

#include <QFile>
#include <QQmlApplicationEngine>
#include <QtQml/qqml.h>
#include <QQmlExtensionPlugin>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>

Q_IMPORT_QML_PLUGIN(AscFilesPlugin)

class TestWindow : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        // Whether previews can be sandboxed is found out once, saying so where they can't (as
        // in a container): found out here, before warnings fail the tests (About shows it).
        Sandbox::sandboxed();
        QQuickStyle::setStyle(QStringLiteral("Basic"));
        SystemTheme::registerType();
        AppSettings::registerType();
        qmlRegisterUncreatableType<Transfers>("AscFiles.System", 1, 0, "Transfers", QStringLiteral("The app makes the one Transfers."));
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
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        DesktopApps apps;
        ActionsModel actions(&backend);
        Properties properties(&backend, &apps);
        Preview preview;
        SearchModel search(&backend);
        tabs.openTab(dir.path());
        FolderModel &folders = *tabs.current()->folderModel();
        QSignalSpy opened(&folders, &FolderModel::opened);
        QQmlApplicationEngine engine;
        engine.addImageProvider(QStringLiteral("thumbnail"), new ThumbnailProvider);
        engine.addImageProvider(QStringLiteral("preview"), new PreviewImageProvider);
        engine.addImageProvider(QStringLiteral("appicon"), new AppIconProvider);
        engine.setInitialProperties({{"tabs", QVariant::fromValue(&tabs)},
                                     {"places", QVariant::fromValue(&places)},
                                     {"recent", QVariant::fromValue(&recent)},
                                     {"favorites", QVariant::fromValue(&favorites)},
                                     {"trash", QVariant::fromValue(&trash)},
                                     {"downloads", QVariant::fromValue(&downloads)},
                                     {"jobModel", QVariant::fromValue(&jobs)},
                                     {"transfers", QVariant::fromValue(&transfers)},
                                     {"ops", QVariant::fromValue(&ops)},
                                     {"apps", QVariant::fromValue(&apps)},
                                     {"actions", QVariant::fromValue(&actions)},
                                     {"properties", QVariant::fromValue(&properties)},
                                     {"preview", QVariant::fromValue(&preview)},
                                     {"search", QVariant::fromValue(&search)},
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

        // The preview opens on the current item in its Popup, then as a Panel.
        folders.select(0);
        QMetaObject::invokeMethod(window, [&] { window->setProperty("previewOpen", true); });
        QTRY_VERIFY(window->property("previewOpen").toBool());
        list->setProperty("currentIndex", 1);
        QTRY_COMPARE(preview.name(), folders.data(folders.index(1), FolderModel::NameRole).toString());
        auto *popup = window->findChild<QObject *>(QStringLiteral("previewPopup"));
        QVERIFY(popup);
        QTRY_VERIFY(popup->property("visible").toBool());
        window->setProperty("previewOpen", false);
        QTRY_VERIFY(!popup->property("visible").toBool());
        QCOMPARE(preview.path(), QString());

        // Each view shows without a warning, and selection reaches the rows.
        tabs.current()->setView(1);
        QTRY_VERIFY(window->findChild<QQuickItem *>(QStringLiteral("fileGrid"))->property("count").toInt() == 3);
        tabs.current()->setView(2);
        QTRY_VERIFY(window->findChild<QQuickItem *>(QStringLiteral("columns"))->property("count").toInt() >= 1);
        // The last column lists the folder.
        auto *columns = window->findChild<QQuickItem *>(QStringLiteral("columns"));
        const auto lastColumnCount = [&] {
            QQuickItem *column = nullptr;
            QMetaObject::invokeMethod(columns, "itemAtIndex", Q_RETURN_ARG(QQuickItem *, column),
                                      Q_ARG(int, columns->property("count").toInt() - 1));
            auto *entries = column ? column->findChild<QQuickItem *>(QStringLiteral("columnEntries")) : nullptr;
            return entries ? entries->property("count").toInt() : -1;
        };
        QTRY_COMPARE_WITH_TIMEOUT(lastColumnCount(), 3, 5000);
        tabs.current()->setView(0);
        folders.selectAll();
        QCOMPARE(folders.selectionCount(), 3);
        folders.clearSelection();

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

        // Properties of a folder: measured, without a warning.
        auto *propertiesDialog = window->findChild<QObject *>(QStringLiteral("propertiesDialog"));
        QVERIFY(propertiesDialog);
        QMetaObject::invokeMethod(propertiesDialog, "show", Q_ARG(QVariant, dir.path()));
        QTRY_VERIFY(propertiesDialog->property("opened").toBool());
        QTRY_VERIFY_WITH_TIMEOUT(!properties.measuring(), 10000);
        QVERIFY2(properties.containsText().startsWith(QStringLiteral("2 files")), qPrintable(properties.containsText()));
        QMetaObject::invokeMethod(propertiesDialog, "close");
        QTRY_VERIFY(!propertiesDialog->property("visible").toBool());

        // Settings opens, and every page shows without a warning.
        auto *settings = window->findChild<QObject *>(QStringLiteral("settingsDialog"));
        QVERIFY(settings);
        QMetaObject::invokeMethod(settings, "open");
        QTRY_VERIFY(settings->property("opened").toBool());
        for (int page = 0; page < settings->property("pages").toList().size(); ++page) {
            settings->setProperty("page", page);
            QTest::qWait(50);
        }
        QMetaObject::invokeMethod(settings, "close");
        QTRY_VERIFY(!settings->property("visible").toBool());

        // Typing in the search field searches, in the Search page.
        auto *field = window->findChild<QQuickItem *>(QStringLiteral("searchField"));
        QVERIFY(field);
        field->forceActiveFocus();
        for (const char key : {'a', '.', 'm'}) {
            QTest::keyClick(window, key);
        }
        QTRY_COMPARE(tabs.current()->page(), Browser::Search);
        QTRY_VERIFY_WITH_TIMEOUT(!search.running() && search.query() == "a.m", 10000);
        tabs.current()->back();
        QTRY_COMPARE(tabs.current()->page(), Browser::Folder);
        field->setProperty("text", "");

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

    void windowsKeepTheirOwnTabs() {
        QTest::failOnWarning(QRegularExpression(QStringLiteral(".*")));
        QTemporaryDir first;
        QTemporaryDir second;
        BackendClient backend(TestSupport::backend());
        backend.start();
        PlacesModel places;
        RecentModel recent;
        FavoritesModel favorites;
        TrashModel trash(&backend);
        FolderModel downloads(&backend);
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        DesktopApps apps;
        ActionsModel actions(&backend);
        QQmlApplicationEngine engine;
        engine.addImageProvider(QStringLiteral("thumbnail"), new ThumbnailProvider);
        engine.addImageProvider(QStringLiteral("preview"), new PreviewImageProvider);
        engine.addImageProvider(QStringLiteral("appicon"), new AppIconProvider);
        Windows windows(&engine, {{"places", QVariant::fromValue(&places)},
                                  {"recent", QVariant::fromValue(&recent)},
                                  {"favorites", QVariant::fromValue(&favorites)},
                                  {"trash", QVariant::fromValue(&trash)},
                                  {"downloads", QVariant::fromValue(&downloads)},
                                  {"jobModel", QVariant::fromValue(&jobs)},
                                  {"transfers", QVariant::fromValue(&transfers)},
                                  {"ops", QVariant::fromValue(&ops)},
                                  {"apps", QVariant::fromValue(&apps)},
                                  {"actions", QVariant::fromValue(&actions)},
                                  {"backend", QVariant::fromValue(&backend)}});
        QQuickWindow *one = windows.open({first.path()});
        QQuickWindow *two = windows.open({second.path(), first.path()});
        QVERIFY(one && two);
        QCOMPARE(windows.count(), 2);
        auto tabsOf = [](QQuickWindow *window) { return window->property("tabs").value<TabModel *>(); };
        QCOMPARE(tabsOf(one)->rowCount(), 1);
        QCOMPARE(tabsOf(two)->rowCount(), 2);
        QCOMPARE(tabsOf(two)->current()->location(), second.path());
        // Only the window used last answers for the shared operations.
        QCOMPARE(windows.current(), one);
        QVERIFY(one->property("foremost").toBool());
        QVERIFY(!two->property("foremost").toBool());

        // The first window keeps the session; the second doesn't.
        tabsOf(one)->openTab(second.path());
        QTRY_VERIFY_WITH_TIMEOUT(QSettings(QSettings::IniFormat, QSettings::UserScope, "asc-files", "asc-files")
                                         .value("session/tabs").toStringList().size() == 2, 5000);
        tabsOf(two)->openTab(second.path());
        QTest::qWait(800);
        QCOMPARE(QSettings(QSettings::IniFormat, QSettings::UserScope, "asc-files", "asc-files").value("session/tabs").toStringList().size(), 2);

        // A closed window goes, with its tabs.
        QPointer<TabModel> closedTabs = tabsOf(two);
        two->close();
        QTRY_COMPARE(windows.count(), 1);
        QTRY_VERIFY(closedTabs.isNull());
        QCOMPARE(windows.current(), one);
        one->close();
        QTRY_COMPARE(windows.count(), 0);
    }
};

QTEST_MAIN(TestWindow)
#include "tst_window.moc"
