// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QJsonObject>
#include <QList>
#include <functional>

class BackendClient;
struct BackendError;

/// The operations started in this session, newest last, as the backend reports them through
/// `job.*` notifications: waiting for their drive, running with byte and file totals, flushing
/// to the drive, finished. A row stays when its job finishes, so a client can show how it ended.
class JobModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int running READ running NOTIFY runningChanged)
public:
    enum Role {
        JobRole = Qt::UserRole + 1,
        LabelRole,
        StateRole,
        ItemsRole,
        DoneRole,
        FailedRole,
        NotAttemptedRole,
        TotalBytesRole,
        BytesRole,
        ProgressRole,
        CancelledRole,
        ErrorRole,
        UndoRole,
        SettledRole,
    };
    Q_ENUM(Role)
    enum State { Starting, Queued, Running, Flushing, Finished };
    Q_ENUM(State)

    explicit JobModel(BackendClient *backend, QObject *parent = nullptr);

    /// Starts a batch (`jobs.start` params as the protocol gives them). `started` gets the job id,
    /// or the error that refused it.
    void start(const QJsonObject &params, const QString &label,
               std::function<void(qint64 job, const BackendError &error)> started = {});
    Q_INVOKABLE void cancel(qint64 job);
    /// Forgets the finished jobs.
    Q_INVOKABLE void clearFinished();

    int running() const;
    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

signals:
    void runningChanged();
    /// A job ended; `job.finished`'s params.
    void finished(qint64 job, const QJsonObject &summary);
    void itemDone(qint64 job, int index, const QJsonObject &outcome);

private:
    struct Job {
        qint64 id = 0;
        QString label;
        State state = Starting;
        int items = 0;
        int done = 0;
        int failed = 0;
        int notAttempted = 0;
        qint64 totalBytes = -1;
        /// Bytes written by each item so far; progress is reported per item.
        QHash<int, qint64> bytes;
        bool cancelled = false;
        QString error;
        qint64 undo = -1;
        bool settled = false;
    };
    void notification(const QString &method, const QJsonObject &params);
    int rowOf(qint64 job) const;
    void changed(int row);

    BackendClient *m_backend;
    QList<Job> m_jobs;
    /// Notices that came before `jobs.start` answered with the job's id.
    QHash<qint64, QList<QPair<QString, QJsonObject>>> m_early;
};
