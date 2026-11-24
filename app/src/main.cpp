// SPDX-License-Identifier: MIT
//! Files: the Qt Quick app over `asc-files-backend`.

#include "backend/BackendClient.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"

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
    FolderModel folders(&backend);
    JobModel jobs(&backend);
    QObject::connect(&backend, &BackendClient::failed, &app, [](const QString &why) {
        qCritical().noquote() << why;
    });

    QQmlApplicationEngine engine;
    engine.setInitialProperties({{QStringLiteral("folderModel"), QVariant::fromValue(&folders)},
                                 {QStringLiteral("jobModel"), QVariant::fromValue(&jobs)},
                                 {QStringLiteral("backend"), QVariant::fromValue(&backend)}});
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(1); },
                     Qt::QueuedConnection);
    engine.loadFromModule("AscFiles", "Main");

    backend.start();
    const QStringList folder = parser.positionalArguments();
    folders.openPath(folder.isEmpty() ? QDir::homePath() : QDir(folder.first()).absolutePath());
    return app.exec();
}
