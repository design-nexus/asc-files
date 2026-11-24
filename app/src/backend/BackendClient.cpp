// SPDX-License-Identifier: MIT
#include "BackendClient.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QStandardPaths>
#include <QTimer>

namespace {

/// Deaths within this window count towards giving up.
constexpr qint64 DeathWindowMs = 60'000;
constexpr int MaxDeaths = 3;

BackendError errorFrom(const QJsonObject &error) {
    BackendError result;
    result.code = error.value(QStringLiteral("code")).toInt();
    result.message = error.value(QStringLiteral("message")).toString();
    result.data = error.value(QStringLiteral("data"));
    result.kind = result.data.toObject().value(QStringLiteral("kind")).toString();
    return result;
}

} // namespace

BackendClient::BackendClient(QString program, QObject *parent) : QObject(parent), m_program(std::move(program)) {}

BackendClient::~BackendClient() {
    m_stopping = true;
    if (m_process && m_process->state() != QProcess::NotRunning) {
        // A clean shutdown lets running jobs send their last events and the undo history settle.
        send(QJsonDocument(QJsonObject{{"jsonrpc", "2.0"}, {"id", ++m_next}, {"method", "shutdown"}})
                 .toJson(QJsonDocument::Compact));
        m_process->closeWriteChannel();
        if (!m_process->waitForFinished(5000)) {
            m_process->kill();
            m_process->waitForFinished(1000);
        }
    }
}

QString BackendClient::defaultProgram() {
    const QString fromEnvironment = qEnvironmentVariable("ASC_FILES_BACKEND");
    if (!fromEnvironment.isEmpty()) {
        return fromEnvironment;
    }
    const QString beside = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("asc-files-backend"));
    if (QFileInfo(beside).isExecutable()) {
        return beside;
    }
    const QString onPath = QStandardPaths::findExecutable(QStringLiteral("asc-files-backend"));
    return onPath.isEmpty() ? QStringLiteral("asc-files-backend") : onPath;
}

void BackendClient::start() {
    if (!m_process && !m_gaveUp) {
        launch();
    }
}

void BackendClient::launch() {
    m_buffer.clear();
    m_initialized = false;
    m_process = new QProcess(this);
    m_process->setProcessChannelMode(QProcess::SeparateChannels);
    m_process->setReadChannel(QProcess::StandardOutput);
    connect(m_process, &QProcess::readyReadStandardOutput, this, &BackendClient::readLines);
    connect(m_process, &QProcess::readyReadStandardError, this, [process = m_process] {
        for (const QByteArray &line : process->readAllStandardError().trimmed().split('\n')) {
            qWarning().noquote() << "asc-files-backend:" << QString::fromUtf8(line);
        }
    });
    connect(m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) {
            m_gaveUp = true;
            failPending(m_process->errorString());
            emit failed(tr("The Files backend could not be started: %1").arg(m_process->errorString()));
        }
    });
    connect(m_process, &QProcess::finished, this, &BackendClient::finished);
    m_process->start(m_program, {});

    // `initialize` goes first; what was asked meanwhile follows it.
    const qint64 id = ++m_next;
    m_pending.insert(id, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            m_gaveUp = true;
            emit failed(tr("The Files backend refused to start a session: %1").arg(error.message));
            return;
        }
        const QJsonObject info = result.toObject();
        m_capabilities.clear();
        for (const QJsonValue &capability : info.value(QStringLiteral("capabilities")).toArray()) {
            m_capabilities.append(capability.toString());
        }
        setReady(true);
        emit ready();
        if (m_relaunch) {
            m_relaunch = false;
            emit restarted();
        }
    });
    const QJsonObject initialize{{"jsonrpc", "2.0"},
                                 {"id", id},
                                 {"method", "initialize"},
                                 {"params", QJsonObject{{"protocolVersion", ProtocolVersion}}}};
    m_process->write(QJsonDocument(initialize).toJson(QJsonDocument::Compact) + '\n');
    m_initialized = true;
    for (const QByteArray &line : std::as_const(m_queued)) {
        m_process->write(line + '\n');
    }
    m_queued.clear();
}

void BackendClient::call(const QString &method, const QJsonObject &params, Reply reply) {
    if (m_gaveUp) {
        if (reply) {
            BackendError error{BackendError::Stopped, {}, tr("The Files backend is not running"), {}};
            QTimer::singleShot(0, this, [reply, error] { reply({}, error); });
        }
        return;
    }
    const qint64 id = ++m_next;
    if (reply) {
        m_pending.insert(id, std::move(reply));
    }
    send(QJsonDocument(QJsonObject{{"jsonrpc", "2.0"}, {"id", id}, {"method", method}, {"params", params}})
             .toJson(QJsonDocument::Compact));
}

void BackendClient::send(const QByteArray &line) {
    if (m_process && m_initialized && m_process->state() != QProcess::NotRunning) {
        m_process->write(line + '\n');
    } else {
        m_queued.append(line);
    }
}

void BackendClient::readLines() {
    m_buffer += m_process->readAllStandardOutput();
    qsizetype newline;
    while ((newline = m_buffer.indexOf('\n')) >= 0) {
        const QByteArray line = m_buffer.left(newline);
        m_buffer.remove(0, newline + 1);
        QJsonParseError parseError;
        const QJsonObject message = QJsonDocument::fromJson(line, &parseError).object();
        if (parseError.error != QJsonParseError::NoError) {
            qWarning() << "asc-files-backend sent a line that is not JSON:" << parseError.errorString();
            continue;
        }
        const QJsonValue id = message.value(QStringLiteral("id"));
        if (!id.isUndefined() && !id.isNull()) {
            if (const Reply reply = m_pending.take(id.toInteger())) {
                const QJsonValue error = message.value(QStringLiteral("error"));
                reply(message.value(QStringLiteral("result")),
                      error.isObject() ? errorFrom(error.toObject()) : BackendError{});
            }
        } else if (message.contains(QStringLiteral("method"))) {
            emit notification(message.value(QStringLiteral("method")).toString(),
                              message.value(QStringLiteral("params")).toObject());
        }
    }
}

void BackendClient::finished() {
    setReady(false);
    m_process->deleteLater();
    m_process = nullptr;
    if (m_stopping || m_gaveUp) {
        return;
    }
    failPending(tr("The Files backend stopped"));
    m_deaths.erase(std::remove_if(m_deaths.begin(), m_deaths.end(),
                                  [](const QDeadlineTimer &death) { return death.hasExpired(); }),
                   m_deaths.end());
    m_deaths.append(QDeadlineTimer(DeathWindowMs));
    if (m_deaths.size() >= MaxDeaths) {
        m_gaveUp = true;
        emit failed(tr("The Files backend stopped %1 times in a minute").arg(m_deaths.size()));
        return;
    }
    m_relaunch = true;
    QTimer::singleShot(250 * m_deaths.size(), this, &BackendClient::launch);
}

void BackendClient::failPending(const QString &why) {
    const auto pending = std::exchange(m_pending, {});
    m_queued.clear();
    const BackendError error{BackendError::Stopped, {}, why, {}};
    for (const Reply &reply : pending) {
        reply({}, error);
    }
}

void BackendClient::setReady(bool ready) {
    if (m_ready != ready) {
        m_ready = ready;
        emit readyChanged();
    }
}
