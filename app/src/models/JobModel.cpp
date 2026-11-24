// SPDX-License-Identifier: MIT
#include "JobModel.h"

#include "../backend/BackendClient.h"

#include <QJsonValue>

JobModel::JobModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {
    connect(m_backend, &BackendClient::notification, this, &JobModel::notification);
    // Jobs the old backend ran are gone with it; what they did is in the undo history.
    connect(m_backend, &BackendClient::restarted, this, [this] {
        for (int row = 0; row < m_jobs.size(); ++row) {
            Job &job = m_jobs[row];
            if (job.state != Finished) {
                job.state = Finished;
                job.error = tr("The Files backend stopped before this finished");
                changed(row);
            }
        }
        emit runningChanged();
    });
}

void JobModel::start(const QJsonObject &params, const QString &label,
                     std::function<void(qint64, const BackendError &)> started) {
    m_backend->call(QStringLiteral("jobs.start"), params,
                    [this, label, started](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            if (started) {
                                started(-1, error);
                            }
                            return;
                        }
                        Job job;
                        job.id = result.toObject().value(QStringLiteral("job")).toInteger();
                        job.label = label;
                        beginInsertRows({}, int(m_jobs.size()), int(m_jobs.size()));
                        m_jobs.append(job);
                        endInsertRows();
                        emit runningChanged();
                        // Notices can beat the reply that names the job.
                        for (const auto &[method, notice] : m_early.take(job.id)) {
                            notification(method, notice);
                        }
                        if (started) {
                            started(job.id, {});
                        }
                    });
}

void JobModel::cancel(qint64 job) { m_backend->call(QStringLiteral("jobs.cancel"), {{"job", job}}); }

void JobModel::clearFinished() {
    for (int row = int(m_jobs.size()) - 1; row >= 0; --row) {
        if (m_jobs.at(row).state == Finished) {
            beginRemoveRows({}, row, row);
            m_jobs.removeAt(row);
            endRemoveRows();
        }
    }
}

int JobModel::running() const {
    return int(std::count_if(m_jobs.cbegin(), m_jobs.cend(), [](const Job &job) { return job.state != Finished; }));
}

int JobModel::rowOf(qint64 job) const {
    for (int row = 0; row < m_jobs.size(); ++row) {
        if (m_jobs.at(row).id == job) {
            return row;
        }
    }
    return -1;
}

void JobModel::changed(int row) { emit dataChanged(index(row), index(row)); }

void JobModel::notification(const QString &method, const QJsonObject &params) {
    if (!method.startsWith(QLatin1String("job."))) {
        return;
    }
    const qint64 id = params.value(QStringLiteral("job")).toInteger();
    const int row = rowOf(id);
    if (row < 0) {
        m_early[id].append({method, params});
        return;
    }
    Job &job = m_jobs[row];
    if (method == QLatin1String("job.queued")) {
        job.state = Queued;
    } else if (method == QLatin1String("job.started")) {
        job.state = Running;
        job.items = params.value(QStringLiteral("total")).toInt();
        const QJsonValue total = params.value(QStringLiteral("totalBytes"));
        job.totalBytes = total.isNull() ? -1 : total.toInteger();
    } else if (method == QLatin1String("job.progress")) {
        job.bytes.insert(params.value(QStringLiteral("item")).toInt(), params.value(QStringLiteral("bytes")).toInteger());
    } else if (method == QLatin1String("job.flushing")) {
        job.state = Flushing;
    } else if (method == QLatin1String("job.itemDone")) {
        const QJsonObject outcome = params.value(QStringLiteral("outcome")).toObject();
        const QString status = outcome.value(QStringLiteral("status")).toString();
        if (status == QLatin1String("done")) {
            ++job.done;
        } else if (status == QLatin1String("failed")) {
            ++job.failed;
            if (job.error.isEmpty()) {
                job.error = outcome.value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString();
            }
        } else {
            ++job.notAttempted;
        }
        emit itemDone(id, params.value(QStringLiteral("index")).toInt(), outcome);
    } else if (method == QLatin1String("job.finished")) {
        job.state = Finished;
        job.cancelled = params.value(QStringLiteral("cancelled")).toBool();
        const QJsonValue undo = params.value(QStringLiteral("undo"));
        job.undo = undo.isNull() ? -1 : undo.toInteger();
        job.settled = params.value(QStringLiteral("settled")).toBool();
        changed(row);
        emit runningChanged();
        emit finished(id, params);
        return;
    }
    changed(row);
}

int JobModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_jobs.size()); }

QVariant JobModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_jobs.size()) {
        return {};
    }
    const Job &job = m_jobs.at(index.row());
    qint64 bytes = 0;
    for (const qint64 itemBytes : job.bytes) {
        bytes += itemBytes;
    }
    switch (role) {
    case JobRole:
        return job.id;
    case LabelRole:
        return job.label;
    case StateRole:
        return job.state;
    case ItemsRole:
        return job.items;
    case DoneRole:
        return job.done;
    case FailedRole:
        return job.failed;
    case NotAttemptedRole:
        return job.notAttempted;
    case TotalBytesRole:
        return job.totalBytes;
    case BytesRole:
        return bytes;
    case ProgressRole:
        // By bytes when the batch measured them, else by items; -1 while unknown.
        if (job.state == Finished) {
            return 1.0;
        }
        if (job.totalBytes > 0) {
            return qMin(1.0, double(bytes) / double(job.totalBytes));
        }
        return job.items > 0 ? double(job.done + job.failed + job.notAttempted) / job.items : -1.0;
    case CancelledRole:
        return job.cancelled;
    case ErrorRole:
        return job.error;
    case UndoRole:
        return job.undo;
    case SettledRole:
        return job.settled;
    default:
        return {};
    }
}

QHash<int, QByteArray> JobModel::roleNames() const {
    return {{JobRole, "job"},
            {LabelRole, "label"},
            {StateRole, "state"},
            {ItemsRole, "items"},
            {DoneRole, "done"},
            {FailedRole, "failed"},
            {NotAttemptedRole, "notAttempted"},
            {TotalBytesRole, "totalBytes"},
            {BytesRole, "bytes"},
            {ProgressRole, "progress"},
            {CancelledRole, "cancelled"},
            {ErrorRole, "error"},
            {UndoRole, "undo"},
            {SettledRole, "settled"}};
}
