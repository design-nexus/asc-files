// SPDX-License-Identifier: MIT
#include "FavoritesModel.h"

#include <QDir>
#include <QFile>
#include <QSaveFile>
#include <QStandardPaths>
#include <QUrl>

namespace {

QString configDir() { return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation); }

} // namespace

FavoritesModel::FavoritesModel(QObject *parent) : ItemListModel(parent) {
    const QString legacy = configDir() + QStringLiteral("/nyx-files/favorites");
    if (!QFile::exists(filePath()) && QFile::exists(legacy)) {
        QDir().mkpath(QFileInfo(filePath()).path());
        QFile::copy(legacy, filePath());
    }
    QDir().mkpath(QFileInfo(filePath()).path());
    m_watch.addPath(QFileInfo(filePath()).path());
    connect(&m_watch, &QFileSystemWatcher::directoryChanged, this, &FavoritesModel::reload);
    reload();
}

QString FavoritesModel::filePath() { return configDir() + QStringLiteral("/asc-files/favorites"); }

QStringList FavoritesModel::read() const {
    QFile file(filePath());
    QStringList paths;
    if (!file.open(QIODevice::ReadOnly)) {
        return paths;
    }
    for (const QByteArray &line : file.readAll().split('\n')) {
        const QUrl url(QString::fromUtf8(line.trimmed()));
        // Only local items: legacy also starred places on shares, which Files reaches later (B7).
        if (url.isLocalFile() && !paths.contains(url.toLocalFile())) {
            paths.append(url.toLocalFile());
        }
    }
    return paths;
}

void FavoritesModel::write(const QStringList &paths) {
    QSaveFile file(filePath());
    if (!file.open(QIODevice::WriteOnly)) {
        qWarning() << "Files could not save favorites:" << file.errorString();
        return;
    }
    for (const QString &path : paths) {
        file.write(QUrl::fromLocalFile(path).toEncoded() + '\n');
    }
    file.commit();
    reload();
}

void FavoritesModel::reload() {
    const QStringList paths = read();
    if (paths == m_paths && rowCount() == paths.size()) {
        return;
    }
    m_paths = paths;
    QList<Item> items;
    for (const QString &path : paths) {
        items.append({path, {}, {}, {}});
    }
    setItems(items);
}

void FavoritesModel::add(const QString &path) {
    if (!m_paths.contains(path)) {
        write(m_paths + QStringList{path});
    }
}

void FavoritesModel::remove(const QString &path) {
    QStringList paths = m_paths;
    if (paths.removeAll(path) > 0) {
        write(paths);
    }
}

bool FavoritesModel::contains(const QString &path) const { return m_paths.contains(path); }
