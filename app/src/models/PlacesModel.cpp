// SPDX-License-Identifier: MIT
#include "PlacesModel.h"

#include "../desktop/Disks.h"

#include "Browser.h"

#include <QDir>
#include <QFileInfo>
#include <QLocale>
#include <QSettings>
#include <QStandardPaths>
#include <QStorageInfo>

namespace {

// Icons as 16×16 SVG path data, in Files' line style.
const QString HomeIcon = QStringLiteral("M2.5 7.5 8 3l5.5 4.5V13h-11z");
const QString DesktopIcon =
    QStringLiteral("M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM6 13.5h4M8 11v2.5");
const QString DocumentsIcon = QStringLiteral("M2 4.5h4l1.5 1.5H14v6.5H2zM5 9h6");
const QString DownloadsIcon = QStringLiteral("M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10");
const QString MusicIcon = QStringLiteral(
    "M6 12V3.5l7-1.5v8.5M6 12a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zM13 10.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0z");
const QString PicturesIcon = QStringLiteral(
    "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM2.5 12l4-4 3 "
    "3 2-2 2.5 2.5");
const QString VideosIcon = QStringLiteral(
    "M3.5 4h6A1.5 1.5 0 0 1 11 5.5v5A1.5 1.5 0 0 1 9.5 12h-6A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM11 7l3-2v6l-3-2");
const QString PinnedIcon = QStringLiteral("M2 4.5h4l1.5 1.5H14v6.5H2z");
const QString LockIcon = QStringLiteral("M4.5 7.5v-2a3.5 3.5 0 0 1 7 0v2M3.5 7.5h9v6h-9z");
const QString DriveIcon = QStringLiteral(
    "M3.5 4h9A1.5 1.5 0 0 1 14 5.5v5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM2 8h12M4.5 10h.01");

const QString OverviewIcon = QStringLiteral("M2.5 7.5 8 3l5.5 4.5V13h-11z");
const QString RecentIcon = QStringLiteral("M8 2.5a5.5 5.5 0 1 1 0 11 5.5 5.5 0 0 1 0-11zM8 5v3l2 1.5");
const QString FavoritesIcon = QStringLiteral("M8 2l1.8 3.7 4 .6-2.9 2.8.7 4L8 11.2 4.4 13.1l.7-4L2.2 6.3l4-.6z");
const QString TrashIcon = QStringLiteral("M3 4.5h10M6 4.5V3h4v1.5M4.5 4.5l.7 9h5.6l.7-9");

constexpr int DrivePollMs = 5000;

QSettings settings() {
    return QSettings(QSettings::IniFormat, QSettings::UserScope, QStringLiteral("asc-files"), QStringLiteral("asc-files"));
}

/// Mounts a user plugs in or mounts by hand; system mounts (/boot, /nix/store, …) stay hidden.
bool isUserDrive(const QStorageInfo &volume) {
    const QString root = volume.rootPath();
    return volume.isValid() && volume.isReady() &&
           (root.startsWith(QLatin1String("/run/media/")) || root.startsWith(QLatin1String("/media/")) ||
            root.startsWith(QLatin1String("/mnt/")));
}

} // namespace

PlacesModel::PlacesModel(QObject *parent) : QAbstractListModel(parent) {
    rebuild();
    m_drives.setInterval(DrivePollMs);
    connect(&m_drives, &QTimer::timeout, this, &PlacesModel::refreshDrives);
    m_drives.start();
}

QList<PlacesModel::Place> PlacesModel::pages() const {
    return {{Pages, tr("Overview"), {}, OverviewIcon, -1, {}, Browser::Overview, QStringLiteral("overview")},
            {Pages, tr("Recent"), {}, RecentIcon, -1, {}, Browser::Recent, QStringLiteral("recent")},
            {Pages, tr("Favorites"), {}, FavoritesIcon, -1, {}, Browser::Favorites, QStringLiteral("favorites")}};
}

QList<PlacesModel::Place> PlacesModel::defaults() const {
    QList<Place> places = pages() + folders();
    places.append(trashPlace());
    return places;
}

QList<PlacesModel::Place> PlacesModel::folders() const {
    QList<Place> places{{Folders, tr("Home"), QDir::homePath(), HomeIcon, -1, {}, 0, QStringLiteral("home")}};
    // Named as Files names them (Qt calls the videos folder "Movies").
    const QList<std::tuple<QStandardPaths::StandardLocation, QString, QString, QString>> standard{
        {QStandardPaths::DesktopLocation, tr("Desktop"), DesktopIcon, QStringLiteral("desktop")},
        {QStandardPaths::DocumentsLocation, tr("Documents"), DocumentsIcon, QStringLiteral("documents")},
        {QStandardPaths::DownloadLocation, tr("Downloads"), DownloadsIcon, QStringLiteral("downloads")},
        {QStandardPaths::MusicLocation, tr("Music"), MusicIcon, QStringLiteral("music")},
        {QStandardPaths::PicturesLocation, tr("Pictures"), PicturesIcon, QStringLiteral("pictures")},
        {QStandardPaths::MoviesLocation, tr("Videos"), VideosIcon, QStringLiteral("videos")}};
    for (const auto &[location, name, icon, key] : standard) {
        const QString path = QStandardPaths::writableLocation(location);
        // A folder XDG names that is Home itself, or missing, is left out.
        if (!path.isEmpty() && path != QDir::homePath() && QFileInfo(path).isDir()) {
            places.append({Folders, name, path, icon, -1, {}, 0, key});
        }
    }
    return places;
}

QList<PlacesModel::Place> PlacesModel::pinned() const {
    QList<Place> places;
    for (const QString &path : settings().value(QStringLiteral("sidebar/pinned")).toStringList()) {
        places.append({Pinned, QFileInfo(path).fileName(), path, PinnedIcon, -1, {}, 0, QStringLiteral("pinned:") + path});
    }
    return places;
}

QList<PlacesModel::Place> PlacesModel::drives() const {
    QList<Place> places;
    const auto describe = [](const QStorageInfo &volume, const QString &name) {
        const double total = double(volume.bytesTotal());
        const double used = total > 0 ? 1.0 - double(volume.bytesAvailable()) / total : -1;
        const QString free = tr("%1 free").arg(QLocale().formattedDataSize(volume.bytesAvailable(), 0, QLocale::DataSizeSIFormat));
        return Place{Storage, name, volume.rootPath(), DriveIcon, used, free, 0, QStringLiteral("drive:") + volume.rootPath()};
    };
    places.append(describe(QStorageInfo::root(), tr("System")));
    for (const QStorageInfo &volume : QStorageInfo::mountedVolumes()) {
        if (isUserDrive(volume)) {
            const QString name = volume.displayName().isEmpty() ? QFileInfo(volume.rootPath()).fileName() : volume.displayName();
            Place place = describe(volume, name);
            if (m_disks) {
                const Volume known = m_disks->volumeAt(volume.rootPath());
                place.volume = known.object;
                place.ejectable = known.ejectable || known.canPowerOff;
                place.removable = known.removable;
            }
            places.append(place);
        }
    }
    // What UDisks2 has that isn't mounted: a click mounts it (or asks to unlock it first).
    if (m_disks) {
        for (const Volume &volume : m_disks->volumes()) {
            if (volume.mountPoint.isEmpty()) {
                Place place{Storage, volume.name, {}, volume.locked ? LockIcon : DriveIcon, -1,
                            volume.locked ? tr("Locked") : tr("Not mounted"), 0, QStringLiteral("volume:") + volume.object};
                place.volume = volume.object;
                place.locked = volume.locked;
                place.ejectable = volume.ejectable || volume.canPowerOff;
                place.removable = volume.removable;
                places.append(place);
            }
        }
    }
    return places;
}

PlacesModel::Place PlacesModel::trashPlace() const {
    return {Storage, tr("Trash"), {}, TrashIcon, -1, {}, Browser::Trash, QStringLiteral("trash")};
}

void PlacesModel::rebuild() {
    const QStringList hidden = settings().value(QStringLiteral("sidebar/hidden")).toStringList();
    const auto shown = [&](QList<Place> places) {
        places.removeIf([&](const Place &place) { return hidden.contains(place.key); });
        return places;
    };
    // The folders in the saved order; folders the order does not name follow in theirs.
    QList<Place> folders = shown(this->folders());
    const QStringList order = settings().value(QStringLiteral("sidebar/order")).toStringList();
    std::stable_sort(folders.begin(), folders.end(), [&](const Place &a, const Place &b) {
        const qsizetype at = order.indexOf(a.key), bt = order.indexOf(b.key);
        return (at < 0 ? order.size() : at) < (bt < 0 ? order.size() : bt);
    });
    beginResetModel();
    m_places = shown(pages()) + folders + pinned() + drives();
    // The Trash comes last; refreshDrives counts on it.
    m_trashShown = !hidden.contains(QStringLiteral("trash"));
    if (m_trashShown) {
        m_places.append(trashPlace());
    }
    endResetModel();
}

int PlacesModel::mode() const { return settings().value(QStringLiteral("sidebar/mode"), Full).toInt(); }

void PlacesModel::setMode(int mode) {
    if (mode != this->mode() && mode >= Full && mode <= Hidden) {
        QSettings saved = settings();
        saved.setValue(QStringLiteral("sidebar/mode"), mode);
        emit modeChanged();
    }
}

void PlacesModel::stepMode() { setMode((mode() + 1) % 3); }

bool PlacesModel::coloredIcons() const { return settings().value(QStringLiteral("sidebar/coloredIcons"), true).toBool(); }

void PlacesModel::setColoredIcons(bool colored) {
    if (colored != coloredIcons()) {
        QSettings saved = settings();
        saved.setValue(QStringLiteral("sidebar/coloredIcons"), colored);
        emit appearanceChanged();
    }
}

QVariantMap PlacesModel::sectionColors() const {
    QSettings saved = settings();
    saved.beginGroup(QStringLiteral("sidebar/colors"));
    QVariantMap colors;
    for (const QString &section : saved.childKeys()) {
        colors.insert(section, saved.value(section));
    }
    return colors;
}

void PlacesModel::setSectionColor(int section, const QString &swatch) {
    QSettings saved = settings();
    const QString key = QStringLiteral("sidebar/colors/%1").arg(section);
    if (swatch.isEmpty()) {
        saved.remove(key);
    } else {
        saved.setValue(key, swatch);
    }
    emit appearanceChanged();
}

void PlacesModel::setHidden(const QString &key, bool hidden) {
    QStringList keys = settings().value(QStringLiteral("sidebar/hidden")).toStringList();
    const bool known = std::any_of(m_places.cbegin(), m_places.cend(), [&](const Place &place) { return place.key == key; }) ||
                       std::any_of(defaults().cbegin(), defaults().cend(), [&](const Place &place) { return place.key == key; });
    if (!known || keys.contains(key) == hidden || !hideable(key)) {
        return;
    }
    if (hidden) {
        keys.append(key);
    } else {
        keys.removeAll(key);
    }
    QSettings saved = settings();
    saved.setValue(QStringLiteral("sidebar/hidden"), keys);
    saved.sync();
    rebuild();
    emit hiddenChanged();
}

QVariantList PlacesModel::defaultPlaces() const {
    const QStringList hidden = settings().value(QStringLiteral("sidebar/hidden")).toStringList();
    QVariantList result;
    for (const Place &place : defaults()) {
        result.append(QVariantMap{{QStringLiteral("key"), place.key},
                                  {QStringLiteral("name"), place.name},
                                  {QStringLiteral("section"), int(place.section)},
                                  {QStringLiteral("hidden"), hidden.contains(place.key)}});
    }
    return result;
}

QVariantList PlacesModel::hiddenPlaces() const {
    const QStringList hidden = settings().value(QStringLiteral("sidebar/hidden")).toStringList();
    QVariantList result;
    for (const Place &place : defaults()) {
        if (hidden.contains(place.key)) {
            result.append(QVariantMap{{QStringLiteral("key"), place.key}, {QStringLiteral("name"), place.name}});
        }
    }
    return result;
}

void PlacesModel::moveFolder(const QString &key, const QString &before) {
    QStringList order;
    for (const Place &place : std::as_const(m_places)) {
        if (place.section == Folders) {
            order.append(place.key);
        }
    }
    if (!order.contains(key) || !order.contains(before) || key == before) {
        return;
    }
    order.removeAll(key);
    order.insert(order.indexOf(before), key);
    QSettings saved = settings();
    saved.setValue(QStringLiteral("sidebar/order"), order);
    saved.sync();
    rebuild();
}

void PlacesModel::resetFolderOrder() {
    QSettings saved = settings();
    saved.remove(QStringLiteral("sidebar/order"));
    saved.sync();
    rebuild();
}

bool PlacesModel::hideable(const QString &key) {
    return !key.startsWith(QLatin1String("pinned:")) && !key.startsWith(QLatin1String("drive:"));
}

QStringList PlacesModel::searchRoots() const {
    QStringList roots{QDir::homePath()};
    for (const Place &drive : drives()) {
        if (drive.path != QLatin1String("/") && !drive.path.startsWith(QDir::homePath() + u'/')) {
            roots.append(drive.path);
        }
    }
    return roots;
}

void PlacesModel::refreshDrives() {
    const QList<Place> now = drives();
    // The drives are the last rows; only they are replaced, so the view keeps its place.
    const int first = int(std::find_if(m_places.cbegin(), m_places.cend(), [](const Place &place) {
                              return place.section == Storage;
                          }) - m_places.cbegin());
    // The drives, then the Trash when it is shown, which stays.
    const int old = int(m_places.size()) - (m_trashShown ? 1 : 0) - first;
    bool same = old == now.size();
    for (int i = 0; same && i < old; ++i) {
        same = m_places.at(first + i).key == now.at(i).key;
    }
    if (same) {
        for (int i = 0; i < old; ++i) {
            m_places[first + i] = now.at(i);
        }
        if (old > 0) {
            emit dataChanged(index(first), index(first + old - 1),
                             {UsedRole, FreeTextRole, NameRole, FirstInSectionRole, VolumeRole, LockedRole, EjectableRole, RemovableRole});
        }
        return;
    }
    if (old > 0) {
        beginRemoveRows({}, first, first + old - 1);
        m_places.erase(m_places.begin() + first, m_places.begin() + first + old);
        endRemoveRows();
    }
    beginInsertRows({}, first, first + int(now.size()) - 1);
    for (int i = 0; i < now.size(); ++i) {
        m_places.insert(first + i, now.at(i));
    }
    endInsertRows();
}

void PlacesModel::pin(const QString &path) {
    QStringList paths = settings().value(QStringLiteral("sidebar/pinned")).toStringList();
    if (!paths.contains(path)) {
        paths.append(path);
        savePinned(paths);
    }
}

void PlacesModel::unpin(const QString &path) {
    QStringList paths = settings().value(QStringLiteral("sidebar/pinned")).toStringList();
    if (paths.removeAll(path) > 0) {
        savePinned(paths);
    }
}

bool PlacesModel::isPinned(const QString &path) const {
    return settings().value(QStringLiteral("sidebar/pinned")).toStringList().contains(path);
}

void PlacesModel::savePinned(const QStringList &paths) {
    QSettings saved = settings();
    saved.setValue(QStringLiteral("sidebar/pinned"), paths);
    saved.sync();
    rebuild();
}

int PlacesModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_places.size()); }

QVariant PlacesModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_places.size()) {
        return {};
    }
    const Place &place = m_places.at(index.row());
    switch (role) {
    case SectionRole:
        return int(place.section);
    case NameRole:
        return place.name;
    case PathRole:
        return place.path;
    case IconRole:
        return place.icon;
    case UsedRole:
        return place.used;
    case FreeTextRole:
        return place.freeText;
    case PageRole:
        return place.page;
    case FirstInSectionRole:
        return index.row() == 0 || m_places.at(index.row() - 1).section != place.section;
    case KeyRole:
        return place.key;
    case HideableRole:
        return hideable(place.key);
    case VolumeRole:
        return place.volume;
    case LockedRole:
        return place.locked;
    case EjectableRole:
        return place.ejectable;
    case RemovableRole:
        return place.removable;
    default:
        return {};
    }
}

QHash<int, QByteArray> PlacesModel::roleNames() const {
    return {{SectionRole, "section"}, {NameRole, "name"},   {PathRole, "path"},
            {IconRole, "icon"},       {UsedRole, "used"}, {FreeTextRole, "freeText"},
            {PageRole, "page"}, {FirstInSectionRole, "firstInSection"}, {KeyRole, "key"},
            {HideableRole, "hideable"}, {VolumeRole, "volume"}, {LockedRole, "locked"}, {EjectableRole, "ejectable"}, {RemovableRole, "removable"}};
}

void PlacesModel::setDisks(Disks *disks) {
    m_disks = disks;
    connect(disks, &Disks::changed, this, &PlacesModel::refreshDrives);
    refreshDrives();
}
