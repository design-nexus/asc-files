// SPDX-License-Identifier: MIT
//! Files: the Qt Quick app over `asc-files-backend`.

#include "backend/BackendClient.h"
#include "models/Browser.h"
#include "models/FavoritesModel.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/PlacesModel.h"
#include "models/RecentModel.h"
#include "models/SearchModel.h"
#include "models/TabModel.h"
#include "models/Transfers.h"
#include "ops/Operations.h"
#include "render/Preview.h"
#include "theme/SystemTheme.h"
#include "thumbnails/Thumbnails.h"
#include "models/TrashModel.h"

#include <QCommandLineParser>
#include <QDir>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QtQml/qqml.h>
#include <QQmlExtensionPlugin>
#include <QQuickStyle>
#include <QStandardPaths>

Q_IMPORT_QML_PLUGIN(AscFilesPlugin)

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("asc-files"));
    QGuiApplication::setApplicationDisplayName(QStringLiteral("Files"));
    QGuiApplication::setDesktopFileName(QStringLiteral("asc-files"));
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    QCommandLineParser parser;
    parser.addHelpOption();
    parser.addOptions({{QStringLiteral("backend"), QStringLiteral("The backend program to run."), QStringLiteral("path")}});
    parser.addPositionalArgument(QStringLiteral("folder"), QStringLiteral("The folder to open (Home by default)."));
    parser.process(app);

    BackendClient backend(parser.isSet(QStringLiteral("backend")) ? parser.value(QStringLiteral("backend"))
                                                                  : BackendClient::defaultProgram());
    TabModel tabs(&backend);
    PlacesModel places;
    RecentModel recent;
    FavoritesModel favorites;
    TrashModel trash(&backend);
    // What is new in Downloads, for the Overview.
    FolderModel downloads(&backend);
    downloads.setNewestFirst(true);
    downloads.openPath(QStandardPaths::writableLocation(QStandardPaths::DownloadLocation));
    JobModel jobs(&backend);
    Transfers transfers(&jobs);
    Operations ops(&backend, &jobs);
    Preview preview;
    SearchModel search(&backend);
    QObject::connect(&backend, &BackendClient::failed, &app, [](const QString &why) {
        qCritical().noquote() << why;
    });

    // The Nyx theme, followed live; Theme.qml reads it.
    SystemTheme systemTheme;
    qmlRegisterSingletonInstance("AscFiles.System", 1, 0, "SystemTheme", &systemTheme);
    QQmlApplicationEngine engine;
    engine.addImageProvider(QStringLiteral("thumbnail"), new ThumbnailProvider);
    engine.addImageProvider(QStringLiteral("preview"), new PreviewImageProvider(&preview));
    engine.setInitialProperties({{QStringLiteral("tabs"), QVariant::fromValue(&tabs)},
                                 {QStringLiteral("places"), QVariant::fromValue(&places)},
                                 {QStringLiteral("recent"), QVariant::fromValue(&recent)},
                                 {QStringLiteral("favorites"), QVariant::fromValue(&favorites)},
                                 {QStringLiteral("trash"), QVariant::fromValue(&trash)},
                                 {QStringLiteral("downloads"), QVariant::fromValue(&downloads)},
                                 {QStringLiteral("jobModel"), QVariant::fromValue(&jobs)},
                                 {QStringLiteral("transfers"), QVariant::fromValue(&transfers)},
                                 {QStringLiteral("ops"), QVariant::fromValue(&ops)},
                                 {QStringLiteral("preview"), QVariant::fromValue(&preview)},
                                 {QStringLiteral("search"), QVariant::fromValue(&search)},
                                 {QStringLiteral("backend"), QVariant::fromValue(&backend)}});
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
                     Qt::QueuedConnection);
    engine.loadFromModule("AscFiles", "Main");

    backend.start();
    const QStringList folders = parser.positionalArguments();
    // Without folders to open, the tabs the last session left; else Home.
    if (folders.isEmpty() && !tabs.restoreSession()) {
        tabs.openTab(QDir::homePath());
    }
    for (const QString &folder : folders) {
        tabs.openTab(QDir(folder).absolutePath());
    }
    if (!folders.isEmpty()) {
        tabs.setCurrentIndex(0);
    }
    return app.exec();
}
