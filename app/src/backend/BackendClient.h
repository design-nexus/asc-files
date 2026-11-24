// SPDX-License-Identifier: MIT
#pragma once

#include <QDeadlineTimer>
#include <QHash>
#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QStringList>
#include <functional>

/// A refusal or failure, as the protocol reports it. `kind` is one of the protocol's error kinds
/// for an operation the backend refused (code 1), empty otherwise.
struct BackendError {
    int code = 0;
    QString kind;
    QString message;
    QJsonValue data;

    bool isNull() const { return code == 0; }
    /// The backend process stopped before it answered.
    static constexpr int Stopped = -32000;
};

/// Runs `asc-files-backend` and speaks its protocol (version 2): JSON-RPC 2.0, one message per
/// line on stdin and stdout. Replies are matched by id, whatever order they come in.
///
/// If the process dies, every call waiting on it fails with BackendError::Stopped and the process
/// is started again, a little later each time; `restarted` then says that listings and jobs it
/// held are gone. Three deaths within a minute and it gives up with `failed`.
class BackendClient : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool ready READ isReady NOTIFY readyChanged)
public:
    using Reply = std::function<void(const QJsonValue &result, const BackendError &error)>;
    static constexpr int ProtocolVersion = 2;

    explicit BackendClient(QString program, QObject *parent = nullptr);
    ~BackendClient() override;

    /// Starts the process. Calls made before it is ready are sent once it is.
    void start();
    /// Sends a request; `reply` runs on this object's thread with its answer, unless `context`,
    /// the object it belongs to, is gone by then.
    void call(const QString &method, const QJsonObject &params, const QObject *context, Reply reply);
    /// Sends a request whose reply belongs to no object (or that wants none).
    void call(const QString &method, const QJsonObject &params, Reply reply = {});

    bool isReady() const { return m_ready; }
    QStringList capabilities() const { return m_capabilities; }
    /// The running process's id, or 0 (for tests that stop it).
    qint64 processId() const { return m_process ? m_process->processId() : 0; }

    /// The backend binary to run: `ASC_FILES_BACKEND`, else `asc-files-backend` beside this
    /// program, else the one on PATH.
    static QString defaultProgram();

signals:
    void readyChanged();
    /// The backend answered `initialize`, the first time or after a restart.
    void ready();
    /// The process was started again after it died: listings and jobs it held are gone.
    void restarted();
    void notification(const QString &method, const QJsonObject &params);
    /// The backend cannot be run, or died too often; nothing more will be sent.
    void failed(const QString &why);

private:
    void launch();
    void send(const QByteArray &line);
    void readLines();
    void finished();
    void failPending(const QString &why);
    void setReady(bool ready);

    QString m_program;
    QProcess *m_process = nullptr;
    QByteArray m_buffer;
    qint64 m_next = 0;
    struct Pending {
        Reply reply;
        /// The object the reply belongs to; a reply whose object is gone is dropped.
        QPointer<const QObject> context;
        bool hasContext = false;
    };
    QHash<qint64, Pending> m_pending;
    /// Lines written while the process is starting; sent after `initialize`.
    QList<QByteArray> m_queued;
    bool m_initialized = false;
    bool m_ready = false;
    bool m_stopping = false;
    bool m_gaveUp = false;
    bool m_relaunch = false;
    QStringList m_capabilities;
    QList<QDeadlineTimer> m_deaths;
};
