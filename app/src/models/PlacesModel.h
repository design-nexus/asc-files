// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>

/// The sidebar's places, section by section: the pages at the top (Overview, Recent,
/// Favorites), Folders (Home and the XDG folders that exist), Pinned (folders the user pinned,
/// kept in the settings file), and Storage (the system drive and mounted removable drives, with
/// how full each is, then the Trash). Drives are looked at again every few
/// seconds and when asked, so a stick plugged in shows up.
///
/// The sidebar is the user's to arrange, as in legacy Files: the folders can be put in another
/// order, the default places hidden, each section given a colour, and the sidebar shown full, as
/// an icon rail, or not at all. Each choice is kept in the settings file.
class PlacesModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int mode READ mode WRITE setMode NOTIFY modeChanged)
    Q_PROPERTY(bool coloredIcons READ coloredIcons WRITE setColoredIcons NOTIFY appearanceChanged)
    Q_PROPERTY(QVariantMap sectionColors READ sectionColors NOTIFY appearanceChanged)
    Q_PROPERTY(QVariantList hiddenPlaces READ hiddenPlaces NOTIFY hiddenChanged)
public:
    enum Role {
        SectionRole = Qt::UserRole + 1,
        NameRole,
        PathRole,
        IconRole,
        UsedRole,
        FreeTextRole,
        PageRole,
        FirstInSectionRole,
        KeyRole,
        HideableRole,
    };
    Q_ENUM(Role)
    enum Section { Pages, Folders, Pinned, Storage };
    Q_ENUM(Section)
    /// The sidebar's three states, which the header toggle steps through.
    enum Mode { Full, Compact, Hidden };
    Q_ENUM(Mode)

    explicit PlacesModel(QObject *parent = nullptr);

    Q_INVOKABLE void pin(const QString &path);
    Q_INVOKABLE void unpin(const QString &path);
    Q_INVOKABLE bool isPinned(const QString &path) const;
    Q_INVOKABLE void refreshDrives();
    /// Home and the mounted drives the user plugged in or mounted: where a search goes.
    Q_INVOKABLE QStringList searchRoots() const;

    int mode() const;
    void setMode(int mode);
    /// Full, then the rail, then hidden, then full again.
    Q_INVOKABLE void stepMode();
    bool coloredIcons() const;
    void setColoredIcons(bool colored);
    /// The swatch picked for each section ("1": Folders, "2": Pinned, "3": Storage); a section
    /// without one uses the theme's.
    QVariantMap sectionColors() const;
    Q_INVOKABLE void setSectionColor(int section, const QString &swatch);
    /// Hides or shows a default place (a page, a standard folder, the Trash) by its key.
    Q_INVOKABLE void setHidden(const QString &key, bool hidden);
    /// Every default place, as `[{key, name, section, hidden}]`, for Settings.
    Q_INVOKABLE QVariantList defaultPlaces() const;
    /// The default places hidden now, as `[{key, name}]`, for a menu that shows them again.
    QVariantList hiddenPlaces() const;
    /// Moves the folder `key` to where `before` is, among the Folders.
    Q_INVOKABLE void moveFolder(const QString &key, const QString &before);
    Q_INVOKABLE void resetFolderOrder();

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

signals:
    void modeChanged();
    void appearanceChanged();
    void hiddenChanged();

private:
    struct Place {
        Section section;
        QString name;
        QString path;
        QString icon;
        /// How full a drive is, 0–1; negative for a folder.
        double used = -1;
        QString freeText;
        /// The Browser::Page a page place shows; 0 (a folder) otherwise.
        int page = 0;
        /// What the settings call it: `home`, `recent`, `trash`, `pinned:<path>`, `drive:<root>`.
        QString key;
    };
    /// Every default place that can be hidden, in its default order.
    QList<Place> defaults() const;
    QList<Place> pages() const;
    QList<Place> folders() const;
    QList<Place> pinned() const;
    QList<Place> drives() const;
    Place trashPlace() const;
    void rebuild();
    void savePinned(const QStringList &paths);
    static bool hideable(const QString &key);

    QList<Place> m_places;
    bool m_trashShown = true;
    QTimer m_drives;
};
