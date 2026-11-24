// SPDX-License-Identifier: MIT
#include "FolderViews.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>

FolderViews &FolderViews::shared() {
    static FolderViews views;
    return views;
}

QString FolderViews::filePath() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) +
           QStringLiteral("/asc-files/folder-views.json");
}

FolderViews::FolderViews() {
    QFile file(filePath());
    if (!file.open(QIODevice::ReadOnly)) {
        return;
    }
    const QJsonObject saved = QJsonDocument::fromJson(file.readAll()).object();
    m_defaultView = qBound(int(List), saved.value(QStringLiteral("defaultView")).toInt(List), int(Columns));
    for (const QJsonValue &value : saved.value(QStringLiteral("folders")).toArray()) {
        const QJsonObject folder = value.toObject();
        const QString path = folder.value(QStringLiteral("path")).toString();
        if (path.isEmpty()) {
            continue;
        }
        m_choices.insert(path, {qBound(int(List), folder.value(QStringLiteral("view")).toInt(), int(Columns)),
                                folder.value(QStringLiteral("sort")).toString(QStringLiteral("name")),
                                folder.value(QStringLiteral("descending")).toBool()});
        m_order.append(path);
    }
}

FolderViews::Choice FolderViews::choice(const QString &folder) const {
    const auto found = m_choices.constFind(folder);
    if (found != m_choices.cend()) {
        return *found;
    }
    Choice fresh;
    fresh.view = m_defaultView;
    return fresh;
}

void FolderViews::remember(const QString &folder, const Choice &choice) {
    m_choices.insert(folder, choice);
    m_order.removeAll(folder);
    m_order.append(folder);
    while (m_order.size() > MaxFolders) {
        m_choices.remove(m_order.takeFirst());
    }
    save();
}

void FolderViews::setDefaultView(int view) {
    m_defaultView = qBound(int(List), view, int(Columns));
    save();
}

void FolderViews::save() const {
    QJsonArray folders;
    for (const QString &path : m_order) {
        const Choice &choice = m_choices.value(path);
        folders.append(QJsonObject{{"path", path}, {"view", choice.view}, {"sort", choice.sortKey}, {"descending", choice.descending}});
    }
    QDir().mkpath(QFileInfo(filePath()).path());
    QSaveFile file(filePath());
    if (file.open(QIODevice::WriteOnly)) {
        file.write(QJsonDocument(QJsonObject{{"defaultView", m_defaultView}, {"folders", folders}}).toJson(QJsonDocument::Compact));
        file.commit();
    }
}
