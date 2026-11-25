// SPDX-License-Identifier: MIT
#include "FolderViews.h"

#include "../settings/AppSettings.h"

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
    for (const QJsonValue &value : saved.value(QStringLiteral("folders")).toArray()) {
        const QJsonObject folder = value.toObject();
        const QString path = folder.value(QStringLiteral("path")).toString();
        if (path.isEmpty()) {
            continue;
        }
        m_choices.insert(path, {qBound(int(List), folder.value(QStringLiteral("view")).toInt(), LastView),
                                folder.value(QStringLiteral("sort")).toString(QStringLiteral("name")),
                                folder.value(QStringLiteral("descending")).toBool(),
                                qBound(MinIconSize, folder.value(QStringLiteral("iconSize")).toInt(DefaultIconSize), MaxIconSize)});
        m_order.append(path);
    }
}

FolderViews::Choice FolderViews::choice(const QString &folder) const {
    const auto found = m_choices.constFind(folder);
    if (found != m_choices.cend()) {
        return *found;
    }
    return defaults();
}

FolderViews::Choice FolderViews::defaults() {
    const AppSettings &settings = AppSettings::shared();
    return {settings.defaultView(), settings.defaultSort(), settings.defaultDescending(), DefaultIconSize};
}

void FolderViews::forgetAll() {
    m_choices.clear();
    m_order.clear();
    save();
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

void FolderViews::save() const {
    QJsonArray folders;
    for (const QString &path : m_order) {
        const Choice &choice = m_choices.value(path);
        folders.append(QJsonObject{{"path", path}, {"view", choice.view}, {"sort", choice.sortKey}, {"descending", choice.descending}, {"iconSize", choice.iconSize}});
    }
    QDir().mkpath(QFileInfo(filePath()).path());
    QSaveFile file(filePath());
    if (file.open(QIODevice::WriteOnly)) {
        file.write(QJsonDocument(QJsonObject{{"folders", folders}}).toJson(QJsonDocument::Compact));
        file.commit();
    }
}
