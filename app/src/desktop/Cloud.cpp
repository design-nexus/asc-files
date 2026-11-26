// SPDX-License-Identifier: MIT
#include "Cloud.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

namespace {

const QString Service = QStringLiteral("io.github.design_nexus.nyx.Cloud1");
const QString Path = QStringLiteral("/io/github/design_nexus/nyx/Cloud1");

} // namespace

Cloud::Cloud(QObject *parent) : QObject(parent) {
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        return;
    }
    bus.connect(Service, Path, Service, QStringLiteral("Changed"), this, SLOT(refresh()));
    bus.connect(Service, Path, Service, QStringLiteral("OperationFinished"), this, SLOT(finished(QString, QString, bool, QString)));
    refresh();
}

QVariantList Cloud::accountsFrom(const QByteArray &json) {
    QVariantList accounts;
    for (const QJsonValue &value : QJsonDocument::fromJson(json).array()) {
        const QJsonObject remote = value.toObject();
        if (remote.value("id").toString().isEmpty() || !remote.value("storage_enabled").toBool(true)) {
            continue;
        }
        accounts.append(QVariantMap{{"id", remote.value("id").toString()},
                                    {"name", remote.value("name").toString()},
                                    {"provider", remote.value("provider").toString()},
                                    {"mounted", remote.value("mount_status").toString() == QLatin1String("mounted")},
                                    {"path", remote.value("mount_path").toString()},
                                    {"error", remote.value("error").toString()}});
    }
    return accounts;
}

void Cloud::refresh() {
    // Started on demand by the bus where it is installed; quietly none where it isn't.
    const QDBusMessage message = QDBusMessage::createMethodCall(Service, Path, Service, QStringLiteral("ListRemotes"));
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *watcher) {
        watcher->deleteLater();
        const QDBusMessage reply = watcher->reply();
        const QVariantList accounts = reply.type() == QDBusMessage::ReplyMessage ? accountsFrom(reply.arguments().value(0).toString().toUtf8())
                                                                                 : QVariantList();
        if (accounts != m_accounts) {
            m_accounts = accounts;
            emit changed();
        }
    });
}

void Cloud::call(const QString &method, const QString &id, bool open) {
    QDBusMessage message = QDBusMessage::createMethodCall(Service, Path, Service, method);
    message << id;
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, open](QDBusPendingCallWatcher *watcher) {
        watcher->deleteLater();
        const QDBusMessage reply = watcher->reply();
        if (reply.type() == QDBusMessage::ErrorMessage) {
            emit failed(reply.errorMessage());
        } else if (open) {
            m_opening.append(reply.arguments().value(0).toString());
        }
    });
}

void Cloud::connectAccount(const QString &id) {
    for (const QVariant &account : std::as_const(m_accounts)) {
        const QVariantMap map = account.toMap();
        if (map.value("id") == id && map.value("mounted").toBool()) {
            emit opened(map.value("path").toString());
            return;
        }
    }
    call(QStringLiteral("Mount"), id, true);
}

void Cloud::disconnectAccount(const QString &id) { call(QStringLiteral("Unmount"), id, false); }

void Cloud::finished(const QString &operation, const QString &account, bool success, const QString &message) {
    const bool opening = m_opening.removeOne(operation);
    refresh();
    if (!success) {
        if (opening || !message.isEmpty()) {
            emit failed(message);
        }
        return;
    }
    if (opening) {
        for (const QVariant &known : std::as_const(m_accounts)) {
            if (known.toMap().value("id") == account) {
                emit opened(known.toMap().value("path").toString());
            }
        }
    }
}
