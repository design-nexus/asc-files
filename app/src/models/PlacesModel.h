// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QTimer>

/// The sidebar's places, section by section: Folders (Home and the XDG folders that exist),
/// Pinned (folders the user pinned, kept in the settings file), and Storage (the system drive and
/// mounted removable drives, with how full each is). Drives are looked at again every few
/// seconds and when asked, so a stick plugged in shows up.
class PlacesModel : public QAbstractListModel {
    Q_OBJECT
public:
    enum Role { SectionRole = Qt::UserRole + 1, NameRole, PathRole, IconRole, UsedRole, FreeTextRole };
    Q_ENUM(Role)
    enum Section { Folders, Pinned, Storage };
    Q_ENUM(Section)

    explicit PlacesModel(QObject *parent = nullptr);

    Q_INVOKABLE void pin(const QString &path);
    Q_INVOKABLE void unpin(const QString &path);
    Q_INVOKABLE bool isPinned(const QString &path) const;
    Q_INVOKABLE void refreshDrives();

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

private:
    struct Place {
        Section section;
        QString name;
        QString path;
        QString icon;
        /// How full a drive is, 0–1; negative for a folder.
        double used = -1;
        QString freeText;
    };
    QList<Place> folders() const;
    QList<Place> pinned() const;
    QList<Place> drives() const;
    void rebuild();
    void savePinned(const QStringList &paths);

    QList<Place> m_places;
    QTimer m_drives;
};
