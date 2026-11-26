// SPDX-License-Identifier: MIT
//! Files: the Qt Quick app over `asc-files-backend`.

#include "app/Windows.h"
#include "backend/BackendClient.h"
#include "history/FolderPicker.h"
#include "desktop/AppIcons.h"
#include "desktop/DesktopBus.h"
#include "desktop/Cloud.h"
#include "desktop/Disks.h"
#include "desktop/Updates.h"
#include "desktop/Network.h"
#include "desktop/Integrations.h"
#include "portal/ChooserService.h"
#include "portal/Placement.h"
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
#include "render/CodeHighlighter.h"
#include "render/Preview.h"
#include "settings/AppSettings.h"
#include "theme/SystemTheme.h"
#include "thumbnails/Thumbnails.h"
#include "models/TrashModel.h"

#include <QCommandLineParser>
#include <QDBusConnection>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QTranslator>
#include <QtQml/qqml.h>
#include <QQmlExtensionPlugin>
#include <QQuickStyle>
#include <QStandardPaths>
#include <QTimer>

Q_IMPORT_QML_PLUGIN(AscFilesPlugin)

int main(int argc, char *argv[]) {
    QGuiApplication app(argc, argv);
    QGuiApplication::setApplicationName(QStringLiteral("asc-files"));
    QGuiApplication::setApplicationDisplayName(QStringLiteral("Files"));
    QGuiApplication::setDesktopFileName(QStringLiteral("asc-files"));
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    QCommandLineParser parser;
    parser.addHelpOption();
    parser.addOptions({{QStringLiteral("backend"), QStringLiteral("The backend program to run."), QStringLiteral("path")},
                       {QStringLiteral("chooser"), QStringLiteral("Serve the desktop's file chooser (xdg-desktop-portal starts it so).")}});
    parser.addPositionalArgument(QStringLiteral("folder"), QStringLiteral("The folder to open (Home by default)."));
    parser.process(app);

    BackendClient backend(parser.isSet(QStringLiteral("backend")) ? parser.value(QStringLiteral("backend"))
                                                                  : BackendClient::defaultProgram());
    PlacesModel places;
    // The drives UDisks2 knows: plugged in, locked, ejectable.
    // The folders opened, for 10xer's z / Z and ranking.
    Visits visits;
    // A newer Files: looked for a moment after start, at most once a day.
    Updates updates;
    QTimer::singleShot(5000, &updates, [&updates] { updates.check(); });
    Disks disks;
    places.setDisks(&disks);
    // Servers (SFTP, SMB, …) through gvfs.
    Network network;
    places.setNetwork(&network);
    // Cloud accounts through Nyx Cloud, where it runs.
    Cloud cloud;
    places.setCloud(&cloud);
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
    // Settings › Integrations: Files as the desktop's file chooser and file manager.
    Integrations integrations(&backend);
    AppIconProvider::useDesktopTheme();
    QObject::connect(&backend, &BackendClient::failed, &app, [](const QString &why) {
        qCritical().noquote() << why;
    });

    // The Nyx theme, followed live; Theme.qml reads it.
    SystemTheme::registerType();
    AppSettings::registerType();
    Customizations::registerType();
    qmlRegisterType<CodeHighlighter>("AscFiles.System", 1, 0, "CodeHighlighter");
    qmlRegisterType<FolderPicker>("AscFiles.System", 1, 0, "FolderPicker");
    qmlRegisterUncreatableType<Transfers>("AscFiles.System", 1, 0, "Transfers", QStringLiteral("The app makes the one Transfers."));
    // English plural forms, and the user's language where Files has it.
    QTranslator plurals;
    if (plurals.load(QLocale(), QStringLiteral("asc-files"), QStringLiteral("_"), QStringLiteral(":/i18n"))) {
        app.installTranslator(&plurals);
    } else if (plurals.load(QStringLiteral(":/i18n/asc-files_en"))) {
        app.installTranslator(&plurals);
    }
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
                              {QStringLiteral("integrations"), QVariant::fromValue(&integrations)},
                              {QStringLiteral("disks"), QVariant::fromValue(&disks)},
                              {QStringLiteral("network"), QVariant::fromValue(&network)},
                              {QStringLiteral("cloud"), QVariant::fromValue(&cloud)},
                              {QStringLiteral("visits"), QVariant::fromValue(&visits)},
                              {QStringLiteral("updates"), QVariant::fromValue(&updates)},
                              {QStringLiteral("backend"), QVariant::fromValue(&backend)}});

    // The desktop's file chooser: a process of its own, which xdg-desktop-portal starts; it shows a
    // window per request and ends once idle.
    if (parser.isSet(QStringLiteral("chooser"))) {
        QGuiApplication::setQuitOnLastWindowClosed(false);
        // Its own window class, which the compositor's rule centres.
        QGuiApplication::setDesktopFileName(QString::fromLatin1(Portal::Placement::ChooserClass));
        if (const auto socket = Portal::Placement::hyprlandSocket()) {
            Portal::Placement::installCenteringRule(*socket, Portal::Placement::QueryTimeout);
        }
        QDBusConnection bus = QDBusConnection::sessionBus();
        auto *service = new Portal::ChooserService(&app);
        QObject::connect(service, &Portal::ChooserService::chooserWanted, &windows,
                         [&windows](Portal::ChooserJob *job) { windows.openChooser(job, job->folder()); });
        if (!bus.registerObject(QStringLiteral("/org/freedesktop/portal/desktop"), service,
                                QDBusConnection::ExportScriptableSlots | QDBusConnection::ExportScriptableProperties) ||
            !bus.registerService(QStringLiteral("org.freedesktop.impl.portal.desktop.asc_files"))) {
            qCritical("Files: the file chooser's bus name is taken or there is no session bus");
            return 1;
        }
        QTimer idle;
        QObject::connect(&idle, &QTimer::timeout, &app, [service] {
            if (service->tracker().beginShutdownIfIdle(Portal::RequestTracker::Clock::now())) {
                QCoreApplication::quit();
            }
        });
        idle.start(10'000);
        backend.start();
        return app.exec();
    }
    // Server addresses connect and open once connected; the rest are folders.
    QStringList folders;
    QStringList addresses;
    for (const QString &argument : parser.positionalArguments()) {
        (Network::isAddress(argument) ? addresses : folders).append(argument);
    }
    // One Files per session: a later launch hands its folders to the one running.
    if (!DesktopBus::claim(&windows, folders)) {
        return 0;
    }
    backend.start();
    // Without folders to open, the tabs the last session left; else Home.
    if (!windows.open(folders)) {
        return 1;
    }
    for (const QString &address : addresses) {
        network.connectTo(address);
    }
    return app.exec();
}
