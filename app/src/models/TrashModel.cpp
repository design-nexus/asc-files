// SPDX-License-Identifier: MIT
#include "TrashModel.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"

#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QStandardPaths>

TrashModel::TrashModel(BackendClient *backend, QObject *parent) : ItemListModel(parent), m_backend(backend) {
    const QString info = QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) + QStringLiteral("/Trash/info");
    if (QFileInfo(info).isDir()) {
        m_watch.addPath(info);
    }
    connect(&m_watch, &QFileSystemWatcher::directoryChanged, this, &TrashModel::reload);
    connect(m_backend, &BackendClient::notification, this, [this](const QString &method, const QJsonObject &) {
        if (method == QLatin1String("job.finished")) {
            reload();
        }
    });
    connect(m_backend, &BackendClient::ready, this, &TrashModel::reload);
}

void TrashModel::reload() {
    m_backend->call(QStringLiteral("trash.list"), {}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            return;
        }
        struct Entry {
            Item item;
            QString original;
        };
        QList<Entry> entries;
        for (const QJsonValue &value : result.toObject().value(QStringLiteral("entries")).toArray()) {
            const QJsonObject entry = value.toObject();
            const QString original = Wire::display(entry.value(QStringLiteral("original")));
            const QDateTime deleted =
                QDateTime::fromString(entry.value(QStringLiteral("deleted")).toString(), Qt::ISODate);
            entries.append({{QFile::decodeName(Wire::decode(entry.value(QStringLiteral("file")))),
                             QFileInfo(original).fileName(), deleted,
                             QFile::decodeName(Wire::decode(entry.value(QStringLiteral("info")))),
                             QFileInfo(original).path()},
                            original});
        }
        std::stable_sort(entries.begin(), entries.end(),
                         [](const Entry &a, const Entry &b) { return a.item.when > b.item.when; });
        QList<Item> items;
        m_originals.clear();
        for (const Entry &entry : entries) {
            items.append(entry.item);
            m_originals.append(entry.original);
        }
        setItems(items);
    });
}

QString TrashModel::originalAt(int row) const { return row >= 0 && row < m_originals.size() ? m_originals.at(row) : QString(); }
