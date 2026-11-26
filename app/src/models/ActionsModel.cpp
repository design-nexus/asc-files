// SPDX-License-Identifier: MIT
#include "ActionsModel.h"

#include "../backend/BackendClient.h"

#include <QFileInfo>
#include <QJsonArray>
#include <QMimeDatabase>

ActionsModel::ActionsModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {
    connect(m_backend, &BackendClient::notification, this, &ActionsModel::notification);
    connect(m_backend, &BackendClient::ready, this, &ActionsModel::reload);
}

QJsonObject ActionsModel::input(const QString &path) {
    const QFileInfo info(path);
    QJsonObject input{{"path", path}};
    if (info.isDir()) {
        input.insert(QStringLiteral("folder"), true);
    } else {
        input.insert(QStringLiteral("type"), QMimeDatabase().mimeTypeForFile(info).name());
    }
    return input;
}

int ActionsModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_runs.size()); }

int ActionsModel::running() const {
    return int(std::count_if(m_runs.cbegin(), m_runs.cend(), [](const QJsonObject &run) {
        const QString status = run.value(QStringLiteral("status")).toString();
        return status == QLatin1String("queued") || status == QLatin1String("running") || status == QLatin1String("cancelling");
    }));
}

QVariant ActionsModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_runs.size()) {
        return {};
    }
    const QJsonObject &run = m_runs.at(index.row());
    const QString status = run.value(QStringLiteral("status")).toString();
    switch (role) {
    case JobRole: return run.value(QStringLiteral("job")).toInteger();
    case NameRole: return run.value(QStringLiteral("name")).toString();
    case StatusRole: return status;
    case ActiveRole: return status == QLatin1String("queued") || status == QLatin1String("running") || status == QLatin1String("cancelling");
    case FractionRole: {
        const QJsonValue fraction = run.value(QStringLiteral("fraction"));
        return fraction.isDouble() ? fraction.toDouble() : -1.0;
    }
    case StepRole: {
        const QString step = run.value(QStringLiteral("step")).toString();
        const int total = run.value(QStringLiteral("total")).toInt();
        if (!step.isEmpty() || run.value(QStringLiteral("mode")).toString() != QLatin1String("per-item") || total <= 1) {
            return step;
        }
        return tr("%1 of %2").arg(run.value(QStringLiteral("completed")).toInt()).arg(total);
    }
    case MessageRole: return run.value(QStringLiteral("message")).toString();
    case LogRole: return run.value(QStringLiteral("log")).toString();
    case CreatedRole: {
        QStringList created;
        for (const QJsonValue &path : run.value(QStringLiteral("created")).toArray()) {
            created.append(path.toString());
        }
        return created;
    }
    case ElapsedRole: return run.value(QStringLiteral("elapsed")).toInteger();
    default: return {};
    }
}

QHash<int, QByteArray> ActionsModel::roleNames() const {
    return {{JobRole, "job"},         {NameRole, "name"},       {StatusRole, "status"}, {ActiveRole, "active"},
            {FractionRole, "fraction"}, {StepRole, "step"},       {MessageRole, "message"}, {LogRole, "log"},
            {CreatedRole, "created"},   {ElapsedRole, "elapsed"}};
}

void ActionsModel::reload() {
    m_backend->call(QStringLiteral("actions.list"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            return;
        }
        m_actions = result.toObject().value(QStringLiteral("actions")).toArray().toVariantList();
        m_failures = result.toObject().value(QStringLiteral("failures")).toArray().toVariantList();
        emit listChanged();
    });
}

void ActionsModel::offer(const QStringList &paths) {
    const quint64 serial = ++m_offerSerial;
    if (!m_offered.isEmpty()) {
        m_offered.clear();
        emit offeredChanged();
    }
    QJsonArray inputs;
    for (const QString &path : paths) {
        // A path not known yet (a row still loading) is nothing to offer for.
        if (!path.isEmpty()) {
            inputs.append(input(path));
        }
    }
    if (inputs.isEmpty()) {
        return;
    }
    m_backend->call(QStringLiteral("actions.list"), {{"inputs", inputs}}, this,
                    [this, serial](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull() || serial != m_offerSerial) {
                            return;
                        }
                        const QJsonObject listed = result.toObject();
                        QHash<QString, QVariant> byId;
                        for (const QJsonValue &action : listed.value(QStringLiteral("actions")).toArray()) {
                            byId.insert(action.toObject().value(QStringLiteral("id")).toString(), action.toObject().toVariantMap());
                        }
                        m_offered.clear();
                        for (const QJsonValue &id : listed.value(QStringLiteral("matched")).toArray()) {
                            m_offered.append(byId.value(id.toString()));
                        }
                        emit offeredChanged();
                    });
}

void ActionsModel::run(const QString &id, const QStringList &paths, const QString &parent, bool background) {
    QJsonArray inputs;
    for (const QString &path : paths) {
        inputs.append(input(path));
    }
    m_backend->call(QStringLiteral("actions.run"), {{"id", id}, {"inputs", inputs}, {"parent", parent}, {"background", background}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit failed(error.message);
                            return;
                        }
                        emit started(result.toObject().value(QStringLiteral("job")).toInteger());
                    });
}

void ActionsModel::cancel(qint64 job) { m_backend->call(QStringLiteral("actions.cancel"), {{"job", job}}); }

void ActionsModel::clearFinished() {
    for (qsizetype row = m_runs.size() - 1; row >= 0; --row) {
        if (!data(index(int(row)), ActiveRole).toBool()) {
            m_backend->call(QStringLiteral("actions.dismiss"), {{"job", m_runs.at(row).value(QStringLiteral("job"))}});
            beginRemoveRows({}, int(row), int(row));
            m_runs.removeAt(row);
            endRemoveRows();
        }
    }
    emit runsChanged();
}

void ActionsModel::notification(const QString &method, const QJsonObject &params) {
    if (method != QLatin1String("action.job")) {
        return;
    }
    const qint64 job = params.value(QStringLiteral("job")).toInteger();
    for (qsizetype row = 0; row < m_runs.size(); ++row) {
        if (m_runs.at(row).value(QStringLiteral("job")).toInteger() == job) {
            m_runs[row] = params;
            emit dataChanged(index(int(row)), index(int(row)));
            emit runsChanged();
            return;
        }
    }
    // Newest first.
    beginInsertRows({}, 0, 0);
    m_runs.prepend(params);
    endInsertRows();
    emit runsChanged();
}

void ActionsModel::edit(const QString &method, const QJsonObject &params) {
    m_backend->call(method, params, this, [this](const QJsonValue &, const BackendError &error) {
        if (!error.isNull()) {
            emit failed(error.message);
        }
        reload();
    });
}

void ActionsModel::setEnabled(const QString &id, bool enabled) { edit(QStringLiteral("actions.setEnabled"), {{"id", id}, {"enabled", enabled}}); }
void ActionsModel::remove(const QString &id) { edit(QStringLiteral("actions.delete"), {{"id", id}}); }
void ActionsModel::duplicate(const QString &id) { edit(QStringLiteral("actions.duplicate"), {{"id", id}}); }
void ActionsModel::importFrom(const QString &folder) { edit(QStringLiteral("actions.import"), {{"path", folder}}); }

void ActionsModel::exportTo(const QString &id, const QString &folder) {
    m_backend->call(QStringLiteral("actions.export"), {{"id", id}, {"into", folder}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit failed(error.message);
                            return;
                        }
                        emit exported(result.toObject().value(QStringLiteral("path")).toString());
                    });
}

void ActionsModel::read(const QString &id) {
    m_backend->call(QStringLiteral("actions.read"), {{"id", id}}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            emit failed(error.message);
            return;
        }
        emit readDone(result.toObject().toVariantMap());
    });
}

void ActionsModel::write(const QVariantMap &definition, const QVariantMap &script, bool create) {
    QJsonObject params{{"definition", QJsonObject::fromVariantMap(definition)}, {"create", create}};
    if (!script.isEmpty()) {
        params.insert(QStringLiteral("script"), QJsonObject::fromVariantMap(script));
    }
    m_backend->call(QStringLiteral("actions.write"), params, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            emit failed(error.message);
            return;
        }
        reload();
        emit saved(result.toObject().value(QStringLiteral("id")).toString());
    });
}

void ActionsModel::readExamples() {
    m_backend->call(QStringLiteral("actions.examples"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (error.isNull()) {
            emit examplesRead(result.toArray().toVariantList());
        }
    });
}
