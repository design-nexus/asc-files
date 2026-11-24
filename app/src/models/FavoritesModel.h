// SPDX-License-Identifier: MIT
#pragma once

#include "ItemListModel.h"

#include <QFileSystemWatcher>

/// The files and folders starred to keep at hand, oldest first: one `file://` URI per line in
/// `$XDG_CONFIG_HOME/asc-files/favorites`, the format legacy Files used. The first time Files
/// runs without the file, it takes a copy of legacy Files' favorites, if there are any.
class FavoritesModel : public ItemListModel {
    Q_OBJECT
public:
    explicit FavoritesModel(QObject *parent = nullptr);

    Q_INVOKABLE void add(const QString &path);
    Q_INVOKABLE void remove(const QString &path);
    Q_INVOKABLE bool contains(const QString &path) const;
    Q_INVOKABLE void reload();

    static QString filePath();

private:
    QStringList read() const;
    void write(const QStringList &paths);

    QStringList m_paths;
    QFileSystemWatcher m_watch;
};
