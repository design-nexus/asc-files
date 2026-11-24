// SPDX-License-Identifier: MIT
#pragma once

#include "ItemListModel.h"

#include <QFileSystemWatcher>

class BackendClient;

/// What is in the Trash, on every drive, newest first, from the backend's `trash.list`. Each
/// row's extra is the entry's `.trashinfo`, which restoring or deleting it names. The list is
/// read again when the home Trash changes and after any job ends.
class TrashModel : public ItemListModel {
    Q_OBJECT
public:
    explicit TrashModel(BackendClient *backend, QObject *parent = nullptr);
    Q_INVOKABLE void reload();
    /// Where the item at `row` was deleted from.
    Q_INVOKABLE QString originalAt(int row) const;

private:
    BackendClient *m_backend;
    QFileSystemWatcher m_watch;
    QList<QString> m_originals;
};
