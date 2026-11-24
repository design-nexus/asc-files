// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QList>

#include "FolderModel.h"

class BackendClient;

/// Columns view: one column per folder from Home (or the top) down to the tab's folder, each a
/// listing of its own, with the folder the next column shows marked in it. Going to a folder
/// under one already shown keeps the columns they share, listings and all.
class ColumnsModel : public QAbstractListModel {
    Q_OBJECT
public:
    enum Role { FolderModelRole = Qt::UserRole + 1, TitleRole, OpenNameRole };
    Q_ENUM(Role)

    explicit ColumnsModel(BackendClient *backend, QObject *parent = nullptr);

    /// Shows the columns down to `path` (raw bytes); the last column is `current`, the tab's own
    /// listing of that folder, so what is selected there is what the tab has selected.
    void setPath(const QByteArray &path, FolderModel *current);
    FolderModel *column(int index) const;

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

private:
    struct Column {
        QByteArray path;
        FolderModel *folders;
        /// Made for the column; the last column's is the tab's and is not.
        bool owned;
    };
    BackendClient *m_backend;
    QList<Column> m_columns;
};
