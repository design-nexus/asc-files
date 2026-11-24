// SPDX-License-Identifier: MIT
#include "RecentModel.h"

#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QUrl>
#include <QXmlStreamReader>

RecentModel::RecentModel(QObject *parent) : ItemListModel(parent) {
    // Programs replace the record by renaming a new one over it: watch its folder too.
    m_watch.addPath(QFileInfo(recordPath()).path());
    connect(&m_watch, &QFileSystemWatcher::directoryChanged, this, &RecentModel::reload);
    connect(&m_watch, &QFileSystemWatcher::fileChanged, this, &RecentModel::reload);
    reload();
}

QString RecentModel::recordPath() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericDataLocation) + QStringLiteral("/recently-used.xbel");
}

void RecentModel::reload() {
    QFile file(recordPath());
    QList<Item> items;
    if (file.open(QIODevice::ReadOnly)) {
        if (!m_watch.files().contains(file.fileName())) {
            m_watch.addPath(file.fileName());
        }
        QXmlStreamReader xml(&file);
        while (xml.readNextStartElement() || !xml.atEnd()) {
            if (xml.isStartElement() && xml.name() == QLatin1String("bookmark")) {
                const QUrl url(xml.attributes().value(QLatin1String("href")).toString());
                const auto stamp = [&](const char *name) {
                    return QDateTime::fromString(xml.attributes().value(QLatin1String(name)).toString(), Qt::ISODate);
                };
                QDateTime when = stamp("visited");
                when = std::max(when, stamp("modified"));
                if (url.isLocalFile()) {
                    items.append({url.toLocalFile(), {}, when.toLocalTime(), {}});
                }
            }
            if (xml.hasError()) {
                break;
            }
        }
    }
    std::stable_sort(items.begin(), items.end(), [](const Item &a, const Item &b) { return a.when > b.when; });
    if (items.size() > MaxItems) {
        items.resize(MaxItems);
    }
    setItems(items);
}
