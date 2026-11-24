// SPDX-License-Identifier: MIT
//! Files: the Qt Quick app over `asc-files-backend`.

#include "backend/BackendClient.h"
#include "models/Browser.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/PlacesModel.h"
#include "models/TabModel.h"

#include <QCommandLineParser>
#include <QDir>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlExtensionPlugin>
#include <QQuickStyle>

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
    JobModel jobs(&backend);
    QObject::connect(&backend, &BackendClient::failed, &app, [](const QString &why) {
        qCritical().noquote() << why;
    });

    QQmlApplicationEngine engine;
    engine.setInitialProperties({{QStringLiteral("tabs"), QVariant::fromValue(&tabs)},
                                 {QStringLiteral("places"), QVariant::fromValue(&places)},
                                 {QStringLiteral("jobModel"), QVariant::fromValue(&jobs)},
                                 {QStringLiteral("backend"), QVariant::fromValue(&backend)}});
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
                     Qt::QueuedConnection);
    engine.loadFromModule("AscFiles", "Main");

    backend.start();
    const QStringList folders = parser.positionalArguments();
    if (folders.isEmpty()) {
        tabs.openTab(QDir::homePath());
    }
    for (const QString &folder : folders) {
        tabs.openTab(QDir(folder).absolutePath());
    }
    // The window was made before the first tab: start it on the first.
    tabs.setCurrentIndex(0);
    return app.exec();
}
