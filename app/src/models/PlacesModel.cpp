// SPDX-License-Identifier: MIT
#include "PlacesModel.h"

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
    return {{Pages, tr("Overview"), {}, OverviewIcon, -1, {}, Browser::Overview},
            {Pages, tr("Recent"), {}, RecentIcon, -1, {}, Browser::Recent},
            {Pages, tr("Favorites"), {}, FavoritesIcon, -1, {}, Browser::Favorites}};
}

QList<PlacesModel::Place> PlacesModel::folders() const {
    QList<Place> places{{Folders, tr("Home"), QDir::homePath(), HomeIcon}};
    // Named as Files names them (Qt calls the videos folder "Movies").
    const QList<std::tuple<QStandardPaths::StandardLocation, QString, QString>> standard{
        {QStandardPaths::DesktopLocation, tr("Desktop"), DesktopIcon},
        {QStandardPaths::DocumentsLocation, tr("Documents"), DocumentsIcon},
        {QStandardPaths::DownloadLocation, tr("Downloads"), DownloadsIcon},
        {QStandardPaths::MusicLocation, tr("Music"), MusicIcon},
        {QStandardPaths::PicturesLocation, tr("Pictures"), PicturesIcon},
        {QStandardPaths::MoviesLocation, tr("Videos"), VideosIcon}};
    for (const auto &[location, name, icon] : standard) {
        const QString path = QStandardPaths::writableLocation(location);
        // A folder XDG names that is Home itself, or missing, is left out.
        if (!path.isEmpty() && path != QDir::homePath() && QFileInfo(path).isDir()) {
            places.append({Folders, name, path, icon});
        }
    }
    return places;
}

QList<PlacesModel::Place> PlacesModel::pinned() const {
    QList<Place> places;
    for (const QString &path : settings().value(QStringLiteral("sidebar/pinned")).toStringList()) {
        places.append({Pinned, QFileInfo(path).fileName(), path, PinnedIcon});
    }
    return places;
}

QList<PlacesModel::Place> PlacesModel::drives() const {
    QList<Place> places;
    const auto describe = [](const QStorageInfo &volume, const QString &name) {
        const double total = double(volume.bytesTotal());
        const double used = total > 0 ? 1.0 - double(volume.bytesAvailable()) / total : -1;
        const QString free = tr("%1 free").arg(QLocale().formattedDataSize(volume.bytesAvailable(), 0, QLocale::DataSizeSIFormat));
        return Place{Storage, name, volume.rootPath(), DriveIcon, used, free};
    };
    places.append(describe(QStorageInfo::root(), tr("System")));
    for (const QStorageInfo &volume : QStorageInfo::mountedVolumes()) {
        if (isUserDrive(volume)) {
            const QString name = volume.displayName().isEmpty() ? QFileInfo(volume.rootPath()).fileName() : volume.displayName();
            places.append(describe(volume, name));
        }
    }
    return places;
}

void PlacesModel::rebuild() {
    beginResetModel();
    m_places = pages() + folders() + pinned() + drives();
    m_places.append({Storage, tr("Trash"), {}, TrashIcon, -1, {}, Browser::Trash});
    endResetModel();
}

void PlacesModel::refreshDrives() {
    const QList<Place> now = drives();
    // The drives are the last rows; only they are replaced, so the view keeps its place.
    const int first = int(std::find_if(m_places.cbegin(), m_places.cend(), [](const Place &place) {
                              return place.section == Storage;
                          }) - m_places.cbegin());
    // The drives, then the Trash, which stays.
    const int old = int(m_places.size()) - 1 - first;
    bool same = old == now.size();
    for (int i = 0; same && i < old; ++i) {
        same = m_places.at(first + i).path == now.at(i).path;
    }
    if (same) {
        for (int i = 0; i < old; ++i) {
            m_places[first + i] = now.at(i);
        }
        if (old > 0) {
            emit dataChanged(index(first), index(first + old - 1), {UsedRole, FreeTextRole, NameRole, FirstInSectionRole});
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
    default:
        return {};
    }
}

QHash<int, QByteArray> PlacesModel::roleNames() const {
    return {{SectionRole, "section"}, {NameRole, "name"},   {PathRole, "path"},
            {IconRole, "icon"},       {UsedRole, "used"}, {FreeTextRole, "freeText"},
            {PageRole, "page"}, {FirstInSectionRole, "firstInSection"}};
}
