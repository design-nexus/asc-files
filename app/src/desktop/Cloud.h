// SPDX-License-Identifier: MIT
#pragma once

#include <QObject>
#include <QVariantList>

/// Cloud accounts from Nyx Cloud (`io.github.design_nexus.nyx.Cloud1`, rclone mounts in the home
/// folder), as legacy showed them: listed in Storage, connected on a click and opened, and
/// disconnected from their menu. Without the service there are none.
class Cloud : public QObject {
    Q_OBJECT
    /// `[{id, name, provider, mounted, path, error}]`: the accounts with storage.
    Q_PROPERTY(QVariantList accounts READ accounts NOTIFY changed)
public:
    explicit Cloud(QObject *parent = nullptr);

    /// The accounts with storage in Cloud1's `ListRemotes` answer (unknown fields ignored).
    static QVariantList accountsFrom(const QByteArray &json);

    QVariantList accounts() const { return m_accounts; }
    Q_INVOKABLE void refresh();
    /// Mounts the account, then says where it is (`opened`).
    Q_INVOKABLE void connectAccount(const QString &id);
    Q_INVOKABLE void disconnectAccount(const QString &id);

signals:
    void changed();
    void opened(const QString &path);
    void failed(const QString &message);

private slots:
    void finished(const QString &operation, const QString &account, bool success, const QString &message);

private:
    void call(const QString &method, const QString &id, bool open);
    QVariantList m_accounts;
    /// Operations started here whose account opens when they succeed.
    QStringList m_opening;
};
