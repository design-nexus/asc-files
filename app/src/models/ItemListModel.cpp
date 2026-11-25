// SPDX-License-Identifier: MIT
#include "ItemListModel.h"

#include "../settings/AppSettings.h"

#include <QDir>
#include <QFileInfo>
#include <QLocale>
#include <QMimeDatabase>
#include <QPointer>
#include <QThreadPool>

namespace {

struct Stat {
    bool exists = false;
    bool isFolder = false;
    qint64 size = 0;
    QDateTime modified;
};

Stat statOf(const QString &path) {
    const QFileInfo info(path);
    if (!info.exists() && !info.isSymLink()) {
        return {};
    }
    return {true, info.isDir(), info.isDir() ? 0 : info.size(), info.lastModified()};
}

QString whenText(const QDateTime &when) { return AppSettings::shared().modifiedText(when, QDateTime::currentDateTime()); }

} // namespace

ItemListModel::ItemListModel(QObject *parent) : QAbstractListModel(parent) {
    // Dates read differently: word them again.
    connect(&AppSettings::shared(), &AppSettings::dateFormatChanged, this, [this] {
        if (!m_rows.isEmpty()) {
            emit dataChanged(index(0), index(int(m_rows.size()) - 1), {WhenTextRole});
        }
    });
}

void ItemListModel::setItems(const QList<Item> &items) {
    QHash<QString, Row> known;
    for (const Row &row : std::as_const(m_rows)) {
        known.insert(row.item.path, row);
    }
    const int oldCount = int(m_rows.size());
    beginResetModel();
    m_rows.clear();
    for (const Item &item : items) {
        Row row = known.value(item.path);
        row.item = item;
        m_rows.append(row);
    }
    endResetModel();
    if (oldCount != m_rows.size()) {
        emit countChanged();
    }
    const quint64 generation = ++m_generation;
    QStringList paths;
    for (const Item &item : items) {
        paths.append(item.path);
    }
    QPointer<ItemListModel> self(this);
    QThreadPool::globalInstance()->start([self, generation, paths] {
        QList<Stat> stats;
        for (const QString &path : paths) {
            stats.append(statOf(path));
        }
        if (!self) {
            return;
        }
        QMetaObject::invokeMethod(
            self.data(),
            [self, generation, stats] {
                if (!self || generation != self->m_generation || stats.size() != self->m_rows.size()) {
                    return;
                }
                for (int i = 0; i < stats.size(); ++i) {
                    Row &row = self->m_rows[i];
                    row.statted = true;
                    row.exists = stats.at(i).exists;
                    row.isFolder = stats.at(i).isFolder;
                    row.size = stats.at(i).size;
                    row.modified = stats.at(i).modified;
                }
                if (!stats.isEmpty()) {
                    emit self->dataChanged(self->index(0), self->index(int(stats.size()) - 1));
                }
            },
            Qt::QueuedConnection);
    });
}

int ItemListModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_rows.size()); }

QString ItemListModel::pathAt(int row) const { return row >= 0 && row < m_rows.size() ? m_rows.at(row).item.path : QString(); }

QString ItemListModel::nameAt(int row) const { return data(index(row), NameRole).toString(); }

QString ItemListModel::extraAt(int row) const { return row >= 0 && row < m_rows.size() ? m_rows.at(row).item.extra : QString(); }

bool ItemListModel::isFolderAt(int row) const { return row >= 0 && row < m_rows.size() && m_rows.at(row).isFolder; }

QVariant ItemListModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_rows.size()) {
        return {};
    }
    const Row &row = m_rows.at(index.row());
    switch (role) {
    case NameRole:
        return row.item.name.isEmpty() ? QFileInfo(row.item.path).fileName() : row.item.name;
    case PathRole:
        return row.item.path;
    case FolderRole: {
        const QString folder = row.item.shownFolder.isEmpty() ? QFileInfo(row.item.path).path() : row.item.shownFolder;
        const QString home = QDir::homePath();
        return folder == home ? tr("Home") : folder.startsWith(home + u'/') ? u'~' + folder.mid(home.size()) : folder;
    }
    case IsFolderRole:
        return row.isFolder;
    case ExistsRole:
        return row.exists;
    case SizeTextRole:
        return row.statted && row.exists && !row.isFolder
                   ? QLocale().formattedDataSize(row.size, 1, QLocale::DataSizeSIFormat)
                   : QString();
    case KindTextRole: {
        if (!row.statted) {
            return QString();
        }
        if (row.isFolder) {
            return tr("Folder");
        }
        static QMimeDatabase mimes;
        const QMimeType type = mimes.mimeTypeForFile(row.item.path, QMimeDatabase::MatchExtension);
        return type.isDefault() ? tr("File") : type.comment();
    }
    case WhenRole:
        return row.item.when.isValid() ? row.item.when : row.modified;
    case WhenTextRole:
        return whenText(row.item.when.isValid() ? row.item.when : row.modified);
    case ExtraRole:
        return row.item.extra;
    default:
        return {};
    }
}

QHash<int, QByteArray> ItemListModel::roleNames() const {
    return {{NameRole, "name"},         {PathRole, "path"},         {FolderRole, "folder"},
            {IsFolderRole, "isFolder"}, {ExistsRole, "exists"},     {SizeTextRole, "sizeText"},
            {KindTextRole, "kindText"}, {WhenRole, "when"},         {WhenTextRole, "whenText"},
            {ExtraRole, "extra"}};
}
