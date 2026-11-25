// SPDX-License-Identifier: MIT
#include "Sandbox.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QPointer>
#include <QProcess>
#include <QStandardPaths>
#include <QTimer>
#include <sys/resource.h>

namespace Sandbox {

namespace {

constexpr rlim_t MemoryBytes = rlim_t(1) << 30;
constexpr rlim_t CpuSeconds = 10;
constexpr int WallMs = 15000;
constexpr qint64 MaxOutput = 64 * 1024 * 1024;

/// bubblewrap, if it is here and allowed to make namespaces (a container may forbid it); the
/// first use tries it once.
QString bwrap() {
    static const QString usable = [] {
        const QString found = QStandardPaths::findExecutable(QStringLiteral("bwrap"));
        if (found.isEmpty()) {
            return QString();
        }
        QProcess probe;
        probe.start(found, {QStringLiteral("--unshare-all"), QStringLiteral("--ro-bind"), QStringLiteral("/"), QStringLiteral("/"),
                            QStringLiteral("--dev"), QStringLiteral("/dev"), QStringLiteral("true")});
        const bool works = probe.waitForFinished(5000) && probe.exitStatus() == QProcess::NormalExit && probe.exitCode() == 0;
        if (!works) {
            qWarning("Files: bubblewrap cannot make a sandbox here; decoders run in their own process, limited, unsandboxed");
        }
        return works ? found : QString();
    }();
    return usable;
}

/// The command line that runs `program` on `file` inside bubblewrap, or directly without it.
QStringList commandLine(const QString &program, const QStringList &args, const QByteArray &file, QString *executable) {
    QStringList inner;
    const QString input = bwrap().isEmpty() ? QFile::decodeName(file) : QStringLiteral("/input");
    for (const QString &arg : args) {
        inner.append(arg == QLatin1String("{}") ? input : arg);
    }
    if (bwrap().isEmpty()) {
        *executable = program;
        return inner;
    }
    *executable = bwrap();
    QStringList command{QStringLiteral("--unshare-all"), QStringLiteral("--die-with-parent"),
                        QStringLiteral("--new-session"), QStringLiteral("--cap-drop"), QStringLiteral("ALL"),
                        QStringLiteral("--clearenv"), QStringLiteral("--setenv"), QStringLiteral("PATH"), QStringLiteral("/usr/bin"),
                        QStringLiteral("--setenv"), QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("offscreen"),
                        QStringLiteral("--proc"), QStringLiteral("/proc"), QStringLiteral("--dev"), QStringLiteral("/dev"),
                        QStringLiteral("--tmpfs"), QStringLiteral("/tmp")};
    // The system's programs and libraries, read-only; nothing of the user's.
    for (const char *dir : {"/usr", "/lib", "/lib64", "/bin", "/sbin", "/nix/store", "/etc/fonts", "/etc/ld.so.cache"}) {
        command << QStringLiteral("--ro-bind-try") << QString::fromLatin1(dir) << QString::fromLatin1(dir);
    }
    // The program itself, wherever it is (a build folder in a home, say).
    // Where Qt's image plugins are (read-only, under the system's folders bound above).
    const QString plugins = qEnvironmentVariable("QT_PLUGIN_PATH");
    if (!plugins.isEmpty()) {
        command << QStringLiteral("--setenv") << QStringLiteral("QT_PLUGIN_PATH") << plugins;
    }
    command << QStringLiteral("--ro-bind") << program << QStringLiteral("/run/decoder");
    command << QStringLiteral("--ro-bind") << QFile::decodeName(file) << QStringLiteral("/input");
    command << QStringLiteral("/run/decoder") << inner;
    return command;
}

void limit() {
    const rlimit memory{MemoryBytes, MemoryBytes};
    const rlimit cpu{CpuSeconds, CpuSeconds};
    const rlimit noFiles{0, 0};
    ::setrlimit(RLIMIT_AS, &memory);
    ::setrlimit(RLIMIT_CPU, &cpu);
    // Nothing it writes may land in a file.
    ::setrlimit(RLIMIT_FSIZE, &noFiles);
}

QProcess *start(const QString &program, const QStringList &args, const QByteArray &file) {
    QString executable;
    const QStringList command = commandLine(program, args, file, &executable);
    auto *process = new QProcess;
    process->setProgram(executable);
    process->setArguments(command);
    process->setProcessChannelMode(QProcess::SeparateChannels);
    process->setChildProcessModifier(limit);
    // Without the sandbox, the decoder sees only what it needs of the environment.
    QProcessEnvironment environment;
    environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("offscreen"));
    const QString plugins = qEnvironmentVariable("QT_PLUGIN_PATH");
    if (!plugins.isEmpty()) {
        environment.insert(QStringLiteral("QT_PLUGIN_PATH"), plugins);
    }
    process->setProcessEnvironment(environment);
    process->start(QIODevice::ReadOnly);
    return process;
}

QImage fromPng(const QByteArray &bytes) {
    QImage image;
    image.loadFromData(bytes, "PNG");
    return image;
}

} // namespace

bool sandboxed() { return !bwrap().isEmpty(); }

QString program(const QString &name) {
    const QString found = QStandardPaths::findExecutable(name);
    return found.isEmpty() ? QString() : QFileInfo(found).canonicalFilePath();
}

QString renderer() {
    const QString beside = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("asc-files-render"));
    if (QFileInfo(beside).isExecutable()) {
        return QFileInfo(beside).canonicalFilePath();
    }
    const QString fromEnvironment = qEnvironmentVariable("ASC_FILES_RENDER");
    return fromEnvironment.isEmpty() ? program(QStringLiteral("asc-files-render")) : fromEnvironment;
}

void run(const QString &program, const QStringList &args, const QByteArray &file, QObject *context,
         std::function<void(const QByteArray &, const QString &)> done) {
    if (program.isEmpty()) {
        QTimer::singleShot(0, context, [done] { done({}, QCoreApplication::translate("Sandbox", "The decoder is not installed")); });
        return;
    }
    QProcess *process = start(program, args, file);
    process->setParent(context);
    auto *output = new QByteArray;
    QObject::connect(process, &QProcess::readyReadStandardOutput, process, [process, output] {
        *output += process->readAllStandardOutput();
        if (output->size() > MaxOutput) {
            process->kill();
        }
    });
    auto *timer = new QTimer(process);
    timer->setSingleShot(true);
    QObject::connect(timer, &QTimer::timeout, process, [process] { process->kill(); });
    timer->start(WallMs);
    QObject::connect(process, &QProcess::finished, context, [process, output, done](int code, QProcess::ExitStatus status) {
        *output += process->readAllStandardOutput();
        const QString error = status == QProcess::NormalExit && code == 0
                                  ? QString()
                                  : QCoreApplication::translate("Sandbox", "The file could not be read");
        done(error.isEmpty() ? *output : QByteArray(), error);
        delete output;
        process->deleteLater();
    });
    QObject::connect(process, &QProcess::errorOccurred, context, [process, output, done](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) {
            done({}, QCoreApplication::translate("Sandbox", "The decoder could not be started"));
            delete output;
            process->deleteLater();
        }
    });
}

void decodeImage(const QByteArray &file, int size, QObject *context, std::function<void(const QImage &, const QString &)> done) {
    run(renderer(), {QStringLiteral("image"), QStringLiteral("{}"), QString::number(size)}, file, context,
        [done](const QByteArray &out, const QString &error) {
            const QImage image = fromPng(out);
            done(image, image.isNull() && error.isEmpty() ? QCoreApplication::translate("Sandbox", "The file could not be read") : error);
        });
}

QByteArray runNow(const QString &program, const QStringList &args, const QByteArray &file) {
    if (program.isEmpty()) {
        return {};
    }
    std::unique_ptr<QProcess> process(start(program, args, file));
    if (!process->waitForFinished(WallMs)) {
        process->kill();
        process->waitForFinished(1000);
        return {};
    }
    if (process->exitStatus() != QProcess::NormalExit || process->exitCode() != 0) {
        return {};
    }
    const QByteArray out = process->readAllStandardOutput();
    return out.size() > MaxOutput ? QByteArray() : out;
}

QImage decodeImageNow(const QByteArray &file, int size) {
    return fromPng(runNow(renderer(), {QStringLiteral("image"), QStringLiteral("{}"), QString::number(size)}, file));
}

QImage videoFrameNow(const QByteArray &file, int size) {
    return fromPng(runNow(renderer(), {QStringLiteral("video"), QStringLiteral("{}"), QString::number(size)}, file));
}

QImage pdfPageNow(const QByteArray &file, int size) {
    return fromPng(runNow(program(QStringLiteral("pdftoppm")),
                          {QStringLiteral("-png"), QStringLiteral("-f"), QStringLiteral("1"), QStringLiteral("-l"), QStringLiteral("1"),
                           QStringLiteral("-scale-to"), QString::number(size), QStringLiteral("-singlefile"), QStringLiteral("{}")},
                          file));
}

} // namespace Sandbox
