// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "app/Windows.h"
#include "backend/BackendClient.h"
#include "history/FolderPicker.h"
#include "desktop/AppIcons.h"
#include "desktop/DesktopApps.h"
#include "models/ActionsModel.h"
#include "models/Browser.h"
#include "models/Customizations.h"
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
#include "render/CodeHighlighter.h"
#include "render/Preview.h"
#include "render/Sandbox.h"
#include "settings/AppSettings.h"
#include "theme/SystemTheme.h"
#include "thumbnails/Thumbnails.h"

#include <QAccessible>
#include <QQmlContext>
#include <QQmlProperty>
#include <QFile>
#include <QFileInfo>
#include <QQmlApplicationEngine>
#include <QtQml/qqml.h>
#include <QQmlExtensionPlugin>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QScopeGuard>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <functional>

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
        Customizations::registerType();
        qmlRegisterType<CodeHighlighter>("AscFiles.System", 1, 0, "CodeHighlighter");
        qmlRegisterType<FolderPicker>("AscFiles.System", 1, 0, "FolderPicker");
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

        // Every visible control a screen reader lands on says what it is.
        QTRY_VERIFY(folders.isLoaded(0));
        QStringList unnamed;
        int controls = 0;
        // The items as they are drawn (delegates are children of their views' items, not objects).
        QList<QQuickItem *> shown;
        const std::function<void(QQuickItem *)> collect = [&](QQuickItem *parent) {
            for (QQuickItem *child : parent->childItems()) {
                if (child->isVisible() && child->opacity() > 0) {
                    shown.append(child);
                    collect(child);
                }
            }
        };
        collect(window->contentItem());
        for (QQuickItem *item : std::as_const(shown)) {
            QQmlContext *context = qmlContext(item);
            if (!item->isVisible() || item->opacity() == 0 || !context) {
                continue;
            }
            // What the item tells a screen reader (its Accessible attached properties).
            // Buttons are named by Accessible.name, else their text (as Qt reads them out).
            const bool button = item->inherits("QQuickAbstractButton");
            const int role = QQmlProperty(item, QStringLiteral("Accessible.role"), context).read().toInt();
            if (!button && role != QAccessible::Button && role != QAccessible::CheckBox && role != QAccessible::PageTab &&
                role != QAccessible::ListItem) {
                continue;
            }
            ++controls;
            QString name = QQmlProperty(item, QStringLiteral("Accessible.name"), context).read().toString();
            if (name.trimmed().isEmpty() && button) {
                name = item->property("text").toString();
            }
            if (name.trimmed().isEmpty()) {
                unnamed.append(QStringLiteral("%1 %2").arg(QString::fromLatin1(item->metaObject()->className()), item->objectName()));
            }
        }
        QVERIFY2(controls > 10, qPrintable(QString::number(controls)));
        QVERIFY2(unnamed.isEmpty(), qPrintable(unnamed.join(QStringLiteral(", "))));

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

        // The view shortcuts switch views.
        window->requestActivate();
        QTest::keyClick(window, Qt::Key_2, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 1);
        QTest::keyClick(window, Qt::Key_3, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 2);
        QTest::keyClick(window, Qt::Key_1, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 0);
        // Compact List and Gallery show the folder; Gallery previews the item the keyboard is on.
        QTest::keyClick(window, Qt::Key_4, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 3);
        auto *compact = window->findChild<QQuickItem *>(QStringLiteral("fileCompact"));
        QVERIFY(compact);
        QTRY_COMPARE(compact->property("count").toInt(), 3);
        QTest::keyClick(window, Qt::Key_5, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 4);
        auto *strip = window->findChild<QQuickItem *>(QStringLiteral("fileStrip"));
        QVERIFY(strip);
        QTRY_COMPARE(strip->property("count").toInt(), 3);
        // Dual Pane: a second pane; F5 copies the selection into its folder.
        QTest::keyClick(window, Qt::Key_6, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 5);
        QTRY_VERIFY(tabs.current()->second() != nullptr);
        Browser *other = tabs.current()->second();
        other->goPath(dir.filePath("folder"));
        QTRY_COMPARE(other->location(), dir.filePath("folder"));
        QTRY_VERIFY(window->findChild<QQuickItem *>(QStringLiteral("rightPane")));
        QTRY_VERIFY(folders.rowOfName("b.txt") >= 0);
        // F5 works once the window sees the selection (its shortcut is enabled by it). The second
        // pane can take the keyboard as it opens (it does on CI), and the first may still reload,
        // clearing a selection made meanwhile: the first is made the pane in use, and selected in,
        // until both hold.
        QTRY_VERIFY_WITH_TIMEOUT(
            [&] {
                if (window->property("rightActive").toBool()) {
                    window->setProperty("rightActive", false);
                }
                if (window->property("selectionCount").toInt() == 0) {
                    folders.select(folders.rowOfName("b.txt"));
                }
                return window->property("selectionCount").toInt() > 0;
            }(),
            10000);
        QTest::keyClick(window, Qt::Key_F5);
        QTRY_VERIFY2_WITH_TIMEOUT(QFile::exists(dir.filePath("folder/b.txt")),
                                  qPrintable(QStringLiteral("selected %1, right pane active %2")
                                                 .arg(window->property("selectionCount").toInt())
                                                 .arg(window->property("rightActive").toBool())),
                                  10000);
        QVERIFY(QFile::exists(dir.filePath("b.txt")));
        QTest::keyClick(window, Qt::Key_1, Qt::ControlModifier);
        QTRY_COMPARE(tabs.current()->view(), 0);

        // Customize: a colour and an icon show in the list at once, and Reset takes them away.
        auto *customize = window->findChild<QObject *>(QStringLiteral("customizeDialog"));
        QVERIFY(customize);
        const QString folderPath = dir.filePath("folder");
        QMetaObject::invokeMethod(customize, "show", Q_ARG(QVariant, QStringList{folderPath}), Q_ARG(QVariant, "folder"), Q_ARG(QVariant, true));
        QTRY_VERIFY(customize->property("opened").toBool());
        QMetaObject::invokeMethod(customize, "setColor", Q_ARG(QVariant, "green"));
        QMetaObject::invokeMethod(customize, "setIcon", Q_ARG(QVariant, "emoji:🎵"));
        const int folderRow = folders.rowOfName("folder");
        QCOMPARE(folders.data(folders.index(folderRow), FolderModel::CustomColorRole).toString(), QStringLiteral("#30a46c"));
        QCOMPARE(folders.data(folders.index(folderRow), FolderModel::CustomIconRole).toString(), QStringLiteral("emoji:🎵"));
        QMetaObject::invokeMethod(customize, "setIcon", Q_ARG(QVariant, "code"));
        QVERIFY(folders.data(folders.index(folderRow), FolderModel::CustomIconRole).toString().startsWith('M'));
        Customizations::shared().reset({folderPath});
        QCOMPARE(folders.data(folders.index(folderRow), FolderModel::CustomColorRole).toString(), QString());
        QMetaObject::invokeMethod(customize, "close");
        QTRY_VERIFY(!customize->property("visible").toBool());

        // Settings opens, and every page shows without a warning.
        auto *settings = window->findChild<QObject *>(QStringLiteral("settingsDialog"));
        QVERIFY(settings);
        QMetaObject::invokeMethod(settings, "open");
        QTRY_VERIFY(settings->property("opened").toBool());
        for (int page = 0; page < settings->property("pages").toList().size(); ++page) {
            settings->setProperty("page", page);
            QTest::qWait(50);
        }
        // The action editor, with its three tabs.
        auto *editor = window->findChild<QObject *>(QStringLiteral("actionEditor"));
        QVERIFY(editor);
        QMetaObject::invokeMethod(editor, "create");
        QTRY_VERIFY(editor->property("opened").toBool());
        for (int tab = 0; tab < 3; ++tab) {
            editor->setProperty("tab", tab);
            QTest::qWait(50);
        }
        // What it saves is an action the backend takes.
        editor->setProperty("name", QStringLiteral("Say hello"));
        editor->setProperty("runtime", QStringLiteral("bash"));
        editor->setProperty("entrypoint", QStringLiteral("run.sh"));
        editor->setProperty("extensions", QStringLiteral("txt, .MD"));
        editor->setProperty("maxItems", QStringLiteral("3"));
        editor->setProperty("scripts", QVariantMap{{"python", ""}, {"bash", "#!/usr/bin/env bash\necho hello\n"}});
        QCOMPARE(editor->property("actionId").toString(), QStringLiteral("say-hello"));
        QMetaObject::invokeMethod(editor, "save");
        QTRY_VERIFY_WITH_TIMEOUT(!editor->property("visible").toBool(), 10000);
        QTRY_VERIFY(!actions.actions().isEmpty());
        QCOMPARE(actions.actions().first().toMap().value("id").toString(), QStringLiteral("say-hello"));
        actions.remove(QStringLiteral("say-hello"));
        QTRY_VERIFY(actions.actions().isEmpty());
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

    // 10xer mode: a cursor apart from the fill, ranges, Esc a step at a time, chords.
    void tenxerMovesACursorApartFromTheSelection() {
        QTest::failOnWarning(QRegularExpression(QStringLiteral(".*")));
        QTemporaryDir dir;
        for (const char *name : {"a.md", "b.txt", "c.txt"}) {
            QFile file(dir.filePath(QString::fromLatin1(name)));
            QVERIFY(file.open(QIODevice::WriteOnly));
            file.write(QByteArray(name[0] == 'c' ? 300 : 3, 'x'));
        }
        QDir().mkdir(dir.filePath("folder"));
        AppSettings::shared().setTenxer(true);
        const auto off = qScopeGuard([] { AppSettings::shared().setTenxer(false); });

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
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *list = window->findChild<QQuickItem *>(QStringLiteral("fileList"));
        QVERIFY(list);
        QTRY_COMPARE(list->property("count").toInt(), 4);
        QTRY_VERIFY(folders.isLoaded(3));
        auto *keys = qvariant_cast<QObject *>(window->property("tenxer"));
        QVERIFY(keys);
        list->forceActiveFocus();
        const auto cursor = [&] { return list->property("currentIndex").toInt(); };
        const auto press = [&](Qt::Key key, Qt::KeyboardModifiers modifiers = Qt::NoModifier) { QTest::keyClick(window, key, modifiers); };
        const auto typeText = [&](const char *text) {
            for (const char *c = text; *c; ++c) {
                QTest::keyClick(window, *c);
            }
        };
        // The cursor starts on the first item; moving it selects nothing.
        QTRY_COMPARE(cursor(), 0);
        press(Qt::Key_J);
        QCOMPARE(cursor(), 1);
        QCOMPARE(folders.selectionCount(), 0);
        // Space adds the item and moves on; v walks a range from the cursor.
        press(Qt::Key_Space);
        QCOMPARE(folders.selectionCount(), 1);
        QCOMPARE(cursor(), 2);
        press(Qt::Key_V);
        QCOMPARE(keys->property("visual").toInt(), 1);
        press(Qt::Key_J);
        QCOMPARE(folders.selectedRows(), (QList<int>{1, 2, 3}));
        // Walking back gives back what the range had covered.
        press(Qt::Key_K);
        QCOMPARE(folders.selectedRows(), (QList<int>{1, 2}));
        // Esc leaves the range and keeps the fill, then clears it.
        press(Qt::Key_Escape);
        QCOMPARE(keys->property("visual").toInt(), 0);
        QCOMPARE(folders.selectionCount(), 2);
        press(Qt::Key_Escape);
        QCOMPARE(folders.selectionCount(), 0);
        // g g and G: first and last.
        press(Qt::Key_G);
        QCOMPARE(keys->property("chord").toString(), QStringLiteral("g"));
        press(Qt::Key_G);
        QCOMPARE(cursor(), 0);
        press(Qt::Key_G, Qt::ShiftModifier);
        QCOMPARE(cursor(), 3);
        // Ctrl+R inverts.
        press(Qt::Key_R, Qt::ControlModifier);
        QCOMPARE(folders.selectionCount(), 4);
        press(Qt::Key_R, Qt::ControlModifier);
        QCOMPARE(folders.selectionCount(), 0);
        // , S: by size, largest first.
        press(Qt::Key_Comma);
        press(Qt::Key_S, Qt::ShiftModifier);
        QCOMPARE(folders.sortKey(), QStringLiteral("size"));
        QVERIFY(folders.sortDescending());
        // An unknown second key says so.
        press(Qt::Key_G);
        press(Qt::Key_X);
        QCOMPARE(keys->property("flash").toString(), QStringLiteral("Unknown chord"));
        // l enters the folder under the cursor; h comes back with the cursor on it.
        press(Qt::Key_G);
        press(Qt::Key_G);
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("folder"));
        press(Qt::Key_L);
        QTRY_COMPARE(tabs.current()->location(), dir.filePath("folder"));
        press(Qt::Key_H);
        QTRY_COMPARE(tabs.current()->location(), dir.path());
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("folder"));
        // y marks what it put on the clipboard; Y takes it back.
        press(Qt::Key_Y);
        const int marked = cursor();
        QTRY_COMPARE(folders.data(folders.index(marked), FolderModel::ClipMarkRole).toInt(), 1);
        press(Qt::Key_Y, Qt::ShiftModifier);
        QTRY_COMPARE(folders.data(folders.index(marked), FolderModel::ClipMarkRole).toInt(), 0);
        // The footer's prompts: / finds a name from the cursor.
        auto *prompt = window->findChild<QQuickItem *>(QStringLiteral("tenxerPrompt"));
        QVERIFY(prompt);
        press(Qt::Key_Slash);
        QTRY_VERIFY(prompt->hasActiveFocus());
        typeText("b.t");
        press(Qt::Key_Return);
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("b.txt"));
        QVERIFY(list->hasActiveFocus());
        // a makes an item by exactly the name typed; a trailing / makes a folder.
        press(Qt::Key_A);
        QTRY_VERIFY(prompt->hasActiveFocus());
        typeText("new notes/");
        press(Qt::Key_Return);
        QTRY_VERIFY(QFileInfo(dir.filePath("new notes")).isDir());
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("new notes"));
        // A name taken keeps the prompt open, saying so.
        press(Qt::Key_A);
        QTRY_VERIFY(prompt->hasActiveFocus());
        typeText("a.md");
        press(Qt::Key_Return);
        QTRY_VERIFY(!keys->property("promptError").toString().isEmpty());
        press(Qt::Key_Escape);
        QTRY_VERIFY(keys->property("prompt").toString().isEmpty());
        // r renames the cursor's item.
        list->forceActiveFocus();
        press(Qt::Key_Slash);
        typeText("a.md");
        press(Qt::Key_Return);
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("a.md"));
        press(Qt::Key_R);
        QTRY_VERIFY(prompt->hasActiveFocus());
        // The stem is selected: typing replaces it and keeps ".md".
        typeText("alpha");
        press(Qt::Key_Return);
        QTRY_VERIFY(QFile::exists(dir.filePath("alpha.md")));
        // C copies the cursor's item into a folder picked by typing.
        QTRY_VERIFY(list->hasActiveFocus());
        QTRY_COMPARE(folders.pathAt(cursor()), dir.filePath("alpha.md"));
        press(Qt::Key_C, Qt::ShiftModifier);
        QTRY_VERIFY(prompt->hasActiveFocus());
        typeText("notes");
        QTRY_COMPARE(keys->property("candidates").toStringList().value(0), dir.filePath("new notes"));
        press(Qt::Key_Return);
        QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(dir.filePath("new notes/alpha.md")), 10000);
        // f filters as it is typed; Esc in the prompt clears it.
        list->forceActiveFocus();
        press(Qt::Key_F);
        typeText("txt");
        QTRY_COMPARE(list->property("count").toInt(), 2);
        press(Qt::Key_Escape);
        QTRY_COMPARE(list->property("count").toInt(), 5);
        // ; lists what it runs; e on something that isn't an archive says so.
        list->forceActiveFocus();
        press(Qt::Key_Semicolon);
        QCOMPARE(keys->property("chord").toString(), QStringLiteral(";"));
        QVERIFY(keys->property("chordOptions").toList().size() >= 4);
        press(Qt::Key_E);
        QCOMPARE(keys->property("flash").toString(), QStringLiteral("Not an archive"));
        // Ctrl+Shift+M leaves the mode.
        list->forceActiveFocus();
        press(Qt::Key_M, Qt::ControlModifier | Qt::ShiftModifier);
        QTRY_VERIFY(!AppSettings::shared().tenxer());
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

        // A window opened takes the keyboard into its folder: the arrows select.
        QFile file(second.filePath("first.txt"));
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.close();
        tabsOf(two)->current()->folderModel()->refresh();
        QTRY_COMPARE(tabsOf(two)->current()->folderModel()->rowCount(), 1);
        two->requestActivate();
        QVERIFY(QTest::qWaitForWindowActive(two));
        QTest::keyClick(two, Qt::Key_Down);
        QTRY_COMPARE(tabsOf(two)->current()->folderModel()->selectionCount(), 1);

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
