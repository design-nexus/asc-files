// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QDateTime>
#include <QList>

/// A list of items by path that are not one folder's contents: recent files, favorites, the
/// Trash. Each item is looked at (kind, size, time) on a worker thread, so a path on a share
/// that stopped answering cannot hold the window; a row shows what is known until then.
class ItemListModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        PathRole,
        FolderRole,
        IsFolderRole,
        ExistsRole,
        SizeTextRole,
        KindTextRole,
        WhenRole,
        WhenTextRole,
        ExtraRole,
    };
    Q_ENUM(Role)

    struct Item {
        /// Where the item is now.
        QString path;
        /// The name to show; the path's last name when empty.
        QString name;
        /// The time the list is about (last used, deleted); empty for the item's own.
        QDateTime when;
        /// What the list adds (a Trash entry's `.trashinfo`, say).
        QString extra;
        /// The folder to show the item in, when not the one it is in (a Trash entry's origin).
        QString shownFolder;
    };

    explicit ItemListModel(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;
    Q_INVOKABLE QString pathAt(int row) const;
    Q_INVOKABLE bool isFolderAt(int row) const;
    Q_INVOKABLE QString nameAt(int row) const;
    Q_INVOKABLE QString extraAt(int row) const;

signals:
    void countChanged();

protected:
    /// Replaces the list; items keep what was known of them while they are looked at again.
    void setItems(const QList<Item> &items);

private:
    struct Row {
        Item item;
        bool statted = false;
        bool exists = true;
        bool isFolder = false;
        qint64 size = 0;
        QDateTime modified;
    };
    QList<Row> m_rows;
    quint64 m_generation = 0;
};
