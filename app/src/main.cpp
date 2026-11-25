// SPDX-License-Identifier: MIT
//! Files: the Qt Quick app over `asc-files-backend`.

#include "app/Windows.h"
#include "backend/BackendClient.h"
#include "desktop/AppIcons.h"
#include "desktop/DesktopApps.h"
#include "models/ActionsModel.h"
#include "models/Customizations.h"
#include "models/FavoritesModel.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/PlacesModel.h"
#include "models/RecentModel.h"
#include "models/Transfers.h"
#include "ops/Operations.h"
#include "render/Preview.h"
#include "settings/AppSettings.h"
#include "theme/SystemTheme.h"
#include "thumbnails/Thumbnails.h"
#include "models/TrashModel.h"

#include <QCommandLineParser>
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
    PlacesModel places;
    RecentModel recent;
    FavoritesModel favorites;
    TrashModel trash(&backend);
    // What is new in Downloads, for the Overview.
    FolderModel downloads(&backend);
    downloads.setNewestFirst(true);
    downloads.openPath(QStandardPaths::writableLocation(QStandardPaths::DownloadLocation));
    JobModel jobs(&backend);
    Operations ops(&backend, &jobs);
    Transfers transfers(&ops);
    // Programs, for Open With; their icons from the desktop's theme.
    DesktopApps apps;
    // The user's custom actions and their runs.
    ActionsModel actions(&backend);
    AppIconProvider::useDesktopTheme();
    QObject::connect(&backend, &BackendClient::failed, &app, [](const QString &why) {
        qCritical().noquote() << why;
    });

    // The Nyx theme, followed live; Theme.qml reads it.
    SystemTheme::registerType();
    AppSettings::registerType();
    Customizations::registerType();
    qmlRegisterUncreatableType<Transfers>("AscFiles.System", 1, 0, "Transfers", QStringLiteral("The app makes the one Transfers."));
    QQmlApplicationEngine engine;
    engine.addImageProvider(QStringLiteral("thumbnail"), new ThumbnailProvider);
    engine.addImageProvider(QStringLiteral("preview"), new PreviewImageProvider);
    engine.addImageProvider(QStringLiteral("appicon"), new AppIconProvider);
    Windows windows(&engine, {{QStringLiteral("places"), QVariant::fromValue(&places)},
                              {QStringLiteral("recent"), QVariant::fromValue(&recent)},
                              {QStringLiteral("favorites"), QVariant::fromValue(&favorites)},
                              {QStringLiteral("trash"), QVariant::fromValue(&trash)},
                              {QStringLiteral("downloads"), QVariant::fromValue(&downloads)},
                              {QStringLiteral("jobModel"), QVariant::fromValue(&jobs)},
                              {QStringLiteral("transfers"), QVariant::fromValue(&transfers)},
                              {QStringLiteral("ops"), QVariant::fromValue(&ops)},
                              {QStringLiteral("apps"), QVariant::fromValue(&apps)},
                              {QStringLiteral("actions"), QVariant::fromValue(&actions)},
                              {QStringLiteral("backend"), QVariant::fromValue(&backend)}});

    backend.start();
    // Without folders to open, the tabs the last session left; else Home.
    if (!windows.open(parser.positionalArguments())) {
        return 1;
    }
    return app.exec();
}
