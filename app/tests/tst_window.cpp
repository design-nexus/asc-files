// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"

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
        FolderModel folders(&backend);
        JobModel jobs(&backend);
        QQmlApplicationEngine engine;
        engine.setInitialProperties({{"folderModel", QVariant::fromValue(&folders)},
                                     {"jobModel", QVariant::fromValue(&jobs)},
                                     {"backend", QVariant::fromValue(&backend)}});
        engine.loadFromModule("AscFiles", "Main");
        QCOMPARE(engine.rootObjects().size(), 1);
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        QVERIFY(window);
        QSignalSpy opened(&folders, &FolderModel::opened);
        folders.openPath(dir.path());
        QVERIFY(opened.wait(10000));
        auto *list = window->findChild<QQuickItem *>(QStringLiteral("fileList"));
        QVERIFY(list);
        QTRY_COMPARE(list->property("count").toInt(), 3);
        QTRY_VERIFY(folders.isLoaded(0));
        QVERIFY(QTest::qWaitForWindowExposed(window));

        // Tiled narrow, the panel and its list stay inside the window.
        window->resize(800, 900);
        QTRY_COMPARE(window->width(), 800);
        const auto rightEdge = [&] { return list->mapToScene(QPointF(list->width(), 0)).x(); };
        QTRY_VERIFY2(rightEdge() <= window->width() && list->width() > 200, qPrintable(QString::number(rightEdge())));
    }
};

QTEST_MAIN(TestWindow)
#include "tst_window.moc"
