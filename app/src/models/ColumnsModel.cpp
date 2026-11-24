// SPDX-License-Identifier: MIT
#include "ColumnsModel.h"

#include "../backend/Wire.h"

#include <QDir>
#include <QFile>

ColumnsModel::ColumnsModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {}

void ColumnsModel::setPath(const QByteArray &path, FolderModel *current) {
    // The folders from the first column down: from Home when the path is under it, else the top.
    const QByteArray home = QFile::encodeName(QDir::homePath());
    QByteArray start = "/";
    if (path == home || path.startsWith(home + '/')) {
        start = home;
    }
    QList<QByteArray> chain{start};
    for (const QByteArray &name : path.mid(start.size()).split('/')) {
        if (!name.isEmpty()) {
            chain.append(Wire::join(chain.last(), name));
        }
    }
    // The columns already showing the same folders, above the last, stay as they are.
    int same = 0;
    while (same < m_columns.size() && same + 1 < chain.size() && m_columns.at(same).owned &&
           m_columns.at(same).path == chain.at(same)) {
        ++same;
    }
    if (same < m_columns.size()) {
        beginRemoveRows({}, same, int(m_columns.size()) - 1);
        while (m_columns.size() > same) {
            const Column gone = m_columns.takeLast();
            if (gone.owned) {
                gone.folders->deleteLater();
            }
        }
        endRemoveRows();
    }
    beginInsertRows({}, same, int(chain.size()) - 1);
    for (int i = same; i < chain.size(); ++i) {
        if (i + 1 == chain.size()) {
            m_columns.append({chain.at(i), current, false});
        } else {
            auto *folders = new FolderModel(m_backend, this);
            folders->open(chain.at(i));
            m_columns.append({chain.at(i), folders, true});
        }
    }
    endInsertRows();
    // Each column marks the folder the next one shows.
    if (!m_columns.isEmpty()) {
        emit dataChanged(index(0), index(int(m_columns.size()) - 1), {OpenNameRole});
    }
}

FolderModel *ColumnsModel::column(int index) const {
    return index >= 0 && index < m_columns.size() ? m_columns.at(index).folders : nullptr;
}

int ColumnsModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_columns.size()); }

QVariant ColumnsModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_columns.size()) {
        return {};
    }
    const Column &column = m_columns.at(index.row());
    switch (role) {
    case FolderModelRole:
        return QVariant::fromValue(column.folders);
    case TitleRole:
        return column.folders->folderName();
    case OpenNameRole:
        return index.row() + 1 < m_columns.size() ? Wire::display(Wire::baseName(m_columns.at(index.row() + 1).path))
                                                   : QString();
    default:
        return {};
    }
}

QHash<int, QByteArray> ColumnsModel::roleNames() const {
    return {{FolderModelRole, "folderModel"}, {TitleRole, "title"}, {OpenNameRole, "openName"}};
}
