// SPDX-License-Identifier: MIT
#include "FolderModel.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"
#include "../settings/AppSettings.h"
#include "../thumbnails/Thumbnails.h"
#include "Customizations.h"

#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonObject>
#include <QLocale>
#include <QMimeDatabase>

namespace {

/// `drwxr-xr-x` for a folder with mode 0755.
QString symbolicMode(const QString &kind, int mode) {
    QString text;
    text += kind == QLatin1String("folder") ? u'd' : kind == QLatin1String("link") ? u'l' : u'-';
    const char *letters = "rwxrwxrwx";
    for (int bit = 0; bit < 9; ++bit) {
        text += (mode & (0400 >> bit)) ? QChar::fromLatin1(letters[bit]) : u'-';
    }
    return text;
}

/// What a file is, by its name alone, as Files shows it ("Markdown document"): reading contents
/// for every row would stall on a slow drive.
QString kindText(const QString &kind, const QString &name) {
    if (kind == QLatin1String("folder")) {
        return FolderModel::tr("Folder");
    }
    if (kind == QLatin1String("link")) {
        return FolderModel::tr("Link");
    }
    static QMimeDatabase mimes;
    const QMimeType type = mimes.mimeTypeForFile(name, QMimeDatabase::MatchExtension);
    return type.isDefault() ? FolderModel::tr("File") : type.comment();
}

/// When an item changed, in the date format chosen in Settings.
QString modifiedText(const QDateTime &when) { return AppSettings::shared().modifiedText(when, QDateTime::currentDateTime()); }

} // namespace

FolderModel::FolderModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {
    connect(m_backend, &BackendClient::notification, this, &FolderModel::notification);
    connect(&AppSettings::shared(), &AppSettings::dateFormatChanged, this, [this] {
        if (rowCount() > 0) {
            emit dataChanged(index(0), index(rowCount() - 1), {ModifiedTextRole});
        }
    });
    // An item given a colour or icon shows it at once.
    connect(&Customizations::shared(), &Customizations::changed, this, [this](const QStringList &paths) {
        for (const QString &path : paths) {
            const int row = QFileInfo(path).path() == QFile::decodeName(m_folder) ? rowOfName(QFileInfo(path).fileName()) : -1;
            if (row >= 0) {
                emit dataChanged(index(row), index(row), {CustomColorRole, CustomIconRole});
            }
        }
    });
    // A new backend holds no listing: read the folder again from it.
    connect(m_backend, &BackendClient::restarted, this, [this] {
        m_listing = -1;
        if (!m_folder.isEmpty()) {
            open(m_folder);
        }
    });
}

FolderModel::~FolderModel() { close(); }

void FolderModel::close() {
    if (m_listing >= 0) {
        m_backend->call(QStringLiteral("list.close"), {{"listing", m_listing}});
        m_listing = -1;
    }
}

void FolderModel::open(const QByteArray &path) {
    // A filter belongs to its folder: another folder opens unfiltered.
    if (path != m_folder && !m_filter.isEmpty()) {
        m_filter.clear();
        emit filterChanged();
    }
    close();
    const quint64 opening = ++m_opening;
    m_folder = path;
    setLoading(true);
    beginResetModel();
    m_count = 0;
    m_pages.clear();
    m_order.clear();
    m_inFlight.clear();
    m_selected.clear();
    m_pendingRows.clear();
    m_allSelected = false;
    m_anchor = -1;
    endResetModel();
    takeLeftovers(QJsonArray());
    emit folderChanged();
    emit selectionChanged();
    QJsonObject params{{"path", Wire::encode(path)}, {"showHidden", m_showHidden}};
    params.insert(QStringLiteral("sort"), sortParams());
    if (!m_filter.isEmpty()) {
        params.insert(QStringLiteral("filter"), m_filter);
    }
    if (!m_globs.isEmpty()) {
        params.insert(QStringLiteral("globs"), QJsonArray::fromStringList(m_globs));
    }
    m_backend->call(QStringLiteral("list.open"), params, this,
                    [this, opening](const QJsonValue &result, const BackendError &error) {
                        if (opening != m_opening) {
                            // Opened somewhere else meanwhile: let this listing go.
                            if (error.isNull()) {
                                m_backend->call(QStringLiteral("list.close"),
                                                {{"listing", result.toObject().value("listing")}});
                            }
                            return;
                        }
                        setLoading(false);
                        setError(error);
                        if (!error.isNull()) {
                            return;
                        }
                        const QJsonObject listing = result.toObject();
                        m_listing = listing.value(QStringLiteral("listing")).toInteger();
                        m_followsChanges = listing.value(QStringLiteral("followsChanges")).toBool();
                        takeLeftovers(listing.value(QStringLiteral("leftovers")));
                        reread(listing.value(QStringLiteral("generation")).toInteger(),
                               listing.value(QStringLiteral("count")).toInt());
                        emit folderChanged();
                        emit opened(m_count);
                    });
}

void FolderModel::open(const QByteArray &path, const QString &key, bool descending) {
    if (key != m_sortKey || descending != m_sortDescending) {
        m_sortKey = key;
        m_sortDescending = descending;
        emit sortChanged();
    }
    open(path);
}

void FolderModel::openPath(const QString &path) { open(QFile::encodeName(path)); }

void FolderModel::openRow(int row) {
    const QByteArray name = rawName(row);
    if (!name.isEmpty()) {
        open(Wire::join(m_folder, name));
    }
}

void FolderModel::openParent() {
    if (!m_folder.isEmpty() && m_folder != "/") {
        open(Wire::parent(m_folder));
    }
}

void FolderModel::refresh() {
    if (m_listing < 0) {
        return;
    }
    const qint64 listing = m_listing;
    m_backend->call(QStringLiteral("list.refresh"), {{"listing", listing}}, this,
                    [this, listing](const QJsonValue &result, const BackendError &error) {
                        if (listing != m_listing || !error.isNull()) {
                            return;
                        }
                        const QJsonObject counted = result.toObject();
                        takeLeftovers(counted.value(QStringLiteral("leftovers")));
                        reread(counted.value(QStringLiteral("generation")).toInteger(),
                               counted.value(QStringLiteral("count")).toInt());
                    });
}

void FolderModel::setShowHidden(bool show) {
    if (m_showHidden == show) {
        return;
    }
    m_showHidden = show;
    emit showHiddenChanged();
    reorder();
}

QJsonObject FolderModel::sortParams() const {
    return {{"key", m_sortKey}, {"descending", m_sortDescending}, {"foldersFirst", m_foldersFirst}};
}

void FolderModel::setSort(const QString &key, bool descending, bool foldersFirst) {
    static const QStringList keys{QStringLiteral("name"), QStringLiteral("size"), QStringLiteral("modified"), QStringLiteral("type")};
    if (!keys.contains(key) || (key == m_sortKey && descending == m_sortDescending && foldersFirst == m_foldersFirst)) {
        return;
    }
    m_sortKey = key;
    m_sortDescending = descending;
    m_foldersFirst = foldersFirst;
    emit sortChanged();
    reorder();
}

void FolderModel::setFilter(const QString &filter) {
    if (filter == m_filter) {
        return;
    }
    m_filter = filter;
    emit filterChanged();
    reorder();
}

void FolderModel::setGlobs(const QStringList &globs) {
    if (globs == m_globs) {
        return;
    }
    m_globs = globs;
    emit globsChanged();
    reorder();
}

void FolderModel::setNewestFirst(bool newest) {
    if (newest) {
        setSort(QStringLiteral("modified"), true, false);
    } else {
        setSort(QStringLiteral("name"), false, true);
    }
}

void FolderModel::reorder() {
    if (m_listing < 0) {
        return;
    }
    const qint64 listing = m_listing;
    m_backend->call(QStringLiteral("list.reorder"),
                    {{"listing", listing}, {"showHidden", m_showHidden}, {"sort", sortParams()}, {"filter", m_filter},
                     {"globs", QJsonArray::fromStringList(m_globs)}},
                    this,
                    [this, listing](const QJsonValue &result, const BackendError &error) {
                        if (listing != m_listing || !error.isNull()) {
                            return;
                        }
                        const QJsonObject counted = result.toObject();
                        takeLeftovers(counted.value(QStringLiteral("leftovers")));
                        reread(counted.value(QStringLiteral("generation")).toInteger(),
                               counted.value(QStringLiteral("count")).toInt());
                    });
}

QString FolderModel::folder() const { return Wire::display(m_folder); }

QString FolderModel::folderName() const {
    return m_folder == "/" ? QStringLiteral("/") : Wire::display(Wire::baseName(m_folder));
}

void FolderModel::reread(qint64 generation, int count) {
    // Rows of the old generation are dropped; the view asks for what it shows again.
    m_pages.clear();
    m_order.clear();
    m_inFlight.clear();
    m_pendingRows.clear();
    m_anchor = -1;
    // The new generation first: a view told of new rows asks for them at once.
    const bool newGeneration = m_generation != generation;
    m_generation = generation;
    const int old = m_count;
    if (count > old) {
        beginInsertRows({}, old, count - 1);
        m_count = count;
        endInsertRows();
    } else if (count < old) {
        beginRemoveRows({}, count, old - 1);
        m_count = count;
        endRemoveRows();
    }
    if (m_count > 0) {
        emit dataChanged(index(0), index(m_count - 1));
    }
    if (newGeneration) {
        emit generationChanged();
    }
}

void FolderModel::notification(const QString &method, const QJsonObject &params) {
    if (!method.startsWith(QLatin1String("list.")) || params.value(QStringLiteral("listing")).toInteger() != m_listing ||
        m_listing < 0) {
        return;
    }
    if (method == QLatin1String("list.changed")) {
        takeLeftovers(params.value(QStringLiteral("leftovers")));
        reread(params.value(QStringLiteral("generation")).toInteger(), params.value(QStringLiteral("count")).toInt());
    } else if (method == QLatin1String("list.gone") || method == QLatin1String("list.unmounted")) {
        const QString left = folder();
        const QByteArray nearest = Wire::decode(params.value(QStringLiteral("nearest")));
        // The backend's listing ended with its folder.
        m_listing = -1;
        open(nearest.isEmpty() ? QByteArrayLiteral("/") : nearest);
        emit folderLeft(left, folder(), method == QLatin1String("list.unmounted"));
    }
}

QByteArray FolderModel::rawName(int row) const {
    const auto page = m_pages.constFind(row / PageSize);
    if (page == m_pages.cend() || row < 0 || row >= m_count) {
        return {};
    }
    return Wire::decode(page->at(row % PageSize).toObject().value(QStringLiteral("name")));
}

QString FolderModel::pathAt(int row) const {
    const QByteArray name = rawName(row);
    return name.isEmpty() ? QString() : QFile::decodeName(Wire::join(m_folder, name));
}

int FolderModel::rowOfName(const QString &name) const {
    for (auto page = m_pages.cbegin(); page != m_pages.cend(); ++page) {
        for (int i = 0; i < page->size(); ++i) {
            if (Wire::display(page->at(i).toObject().value(QStringLiteral("name"))) == name) {
                return page.key() * PageSize + i;
            }
        }
    }
    return -1;
}

bool FolderModel::isFolder(int row) const {
    return row >= 0 && row < m_count && isLoaded(row) && data(index(row), IsFolderRole).toBool();
}

int FolderModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : m_count; }

bool FolderModel::isLoaded(int row) const { return m_pages.contains(row / PageSize); }

QVariant FolderModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_count) {
        return {};
    }
    const int page = index.row() / PageSize;
    const auto found = m_pages.constFind(page);
    if (found == m_pages.cend()) {
        request(page);
        return role == LoadedRole ? QVariant(false) : QVariant();
    }
    // Ask for the neighbouring page before the view reaches it, in whichever direction it goes.
    const int within = index.row() % PageSize;
    if (within >= PageSize * 3 / 4 && (page + 1) * PageSize < m_count && !m_pages.contains(page + 1)) {
        request(page + 1);
    } else if (within < PageSize / 4 && page > 0 && !m_pages.contains(page - 1)) {
        request(page - 1);
    }
    const QJsonObject row = found->at(within).toObject();
    const QString kind = row.value(QStringLiteral("kind")).toString();
    const int mode = row.value(QStringLiteral("mode")).toInt();
    switch (role) {
    case NameRole:
        return Wire::display(row.value(QStringLiteral("name")));
    case KindRole:
        return kind;
    case KindTextRole:
        return kindText(kind, Wire::display(row.value(QStringLiteral("name"))));
    case IsFolderRole:
        return kind == QLatin1String("folder") || row.value(QStringLiteral("linkToFolder")).toBool();
    case HiddenRole:
        return row.value(QStringLiteral("hidden")).toBool();
    case SizeRole:
        return row.value(QStringLiteral("size")).toDouble();
    case SizeTextRole:
        if (kind == QLatin1String("folder") || row.value(QStringLiteral("gone")).toBool()) {
            return QString();
        }
        return QLocale().formattedDataSize(row.value(QStringLiteral("size")).toInteger(), 1,
                                           QLocale::DataSizeSIFormat);
    case ModifiedRole: {
        if (!row.contains(QStringLiteral("modified"))) {
            return {};
        }
        const qint64 ms = row.value(QStringLiteral("modified")).toInteger() * 1000 +
                          row.value(QStringLiteral("modifiedNs")).toInteger() / 1'000'000;
        return QDateTime::fromMSecsSinceEpoch(ms);
    }
    case ModifiedTextRole:
        return row.contains(QStringLiteral("modified")) ? modifiedText(data(index, ModifiedRole).toDateTime()) : QString();
    case ModeOctalRole:
        return row.contains(QStringLiteral("mode")) ? QString::number(mode & 07777, 8) : QString();
    case ModeSymbolicRole:
        return row.contains(QStringLiteral("mode")) ? symbolicMode(kind, mode) : QString();
    case GoneRole:
        return row.value(QStringLiteral("gone")).toBool();
    case LoadedRole:
        return true;
    case SelectedRole:
        return isSelected(index.row());
    case CustomColorRole:
        return Customizations::shared().colorOf(pathAt(index.row()));
    case CustomIconRole:
        return Customizations::shared().iconOf(pathAt(index.row()));
    case ThumbnailRole: {
        const QJsonValue name = row.value(QStringLiteral("name"));
        if (kind != QLatin1String("file") || !Thumbnails::canMake(Wire::display(name))) {
            return QString();
        }
        return QString::fromLatin1(Wire::join(m_folder, Wire::decode(name)).toPercentEncoding());
    }
    default:
        return {};
    }
}

QHash<int, QByteArray> FolderModel::roleNames() const {
    return {{NameRole, "name"},
            {KindRole, "kind"},
            {KindTextRole, "kindText"},
            {IsFolderRole, "isFolder"},
            {HiddenRole, "hidden"},
            {SizeRole, "size"},
            {SizeTextRole, "sizeText"},
            {ModifiedRole, "modified"},
            {ModifiedTextRole, "modifiedText"},
            {ModeOctalRole, "modeOctal"},
            {ModeSymbolicRole, "modeSymbolic"},
            {GoneRole, "gone"},
            {LoadedRole, "loaded"},
            {SelectedRole, "selected"},
            {ThumbnailRole, "thumbnail"},
            {CustomColorRole, "customColor"},
            {CustomIconRole, "customIcon"}};
}

void FolderModel::request(int page) const {
    if (m_listing < 0 || m_inFlight.value(page, -1) == m_generation) {
        return;
    }
    m_inFlight.insert(page, m_generation);
    auto *self = const_cast<FolderModel *>(this);
    const qint64 listing = m_listing;
    const qint64 generation = m_generation;
    m_backend->call(QStringLiteral("list.window"),
                    {{"listing", listing}, {"start", page * PageSize}, {"count", PageSize}}, self,
                    [self, page, listing, generation](const QJsonValue &result, const BackendError &error) {
                        if (listing != self->m_listing || generation != self->m_generation) {
                            return; // Meant for a listing or a read the model has left.
                        }
                        self->m_inFlight.remove(page);
                        if (!error.isNull()) {
                            return;
                        }
                        const QJsonObject window = result.toObject();
                        if (window.value(QStringLiteral("generation")).toInteger() != generation) {
                            // The folder was read again before this window: the change notice
                            // is on its way and will ask for the rows afresh.
                            return;
                        }
                        self->m_pages.insert(page, window.value(QStringLiteral("rows")).toArray());
                        self->m_order.append(page);
                        while (self->m_order.size() > MaxPages) {
                            self->m_pages.remove(self->m_order.takeFirst());
                        }
                        ++self->m_pagesFetched;
                        self->resolvePending(page);
                        const int first = page * PageSize;
                        const int last = qMin(first + PageSize, self->m_count) - 1;
                        if (last >= first) {
                            emit self->dataChanged(self->index(first), self->index(last));
                        }
                    });
}

void FolderModel::setLoading(bool loading) {
    if (m_loading != loading) {
        m_loading = loading;
        emit loadingChanged();
    }
}

void FolderModel::setError(const BackendError &error) {
    const QString message = error.isNull() ? QString() : error.message;
    if (m_error != message || m_errorKind != error.kind) {
        m_error = message;
        m_errorKind = error.kind;
        emit errorChanged();
    }
}

int FolderModel::selectionCount() const {
    if (m_allSelected) {
        return qMax(0, m_count - int(m_selected.size()));
    }
    return int(m_selected.size() + m_pendingRows.size());
}

bool FolderModel::isSelected(int row) const {
    if (row < 0 || row >= m_count) {
        return false;
    }
    if (m_pendingRows.contains(row)) {
        return true;
    }
    const QByteArray name = rawName(row);
    // In "all" mode the set holds the rows taken out.
    return !name.isEmpty() && (m_selected.contains(name) != m_allSelected);
}

void FolderModel::selectionTouched() {
    if (m_count > 0) {
        emit dataChanged(index(0), index(m_count - 1), {SelectedRole});
    }
    emit selectionChanged();
}

QList<int> FolderModel::addNames(int first, int last) {
    QList<int> missing;
    for (int row = qMax(0, first); row <= qMin(last, m_count - 1); ++row) {
        const QByteArray name = rawName(row);
        if (name.isEmpty()) {
            missing.append(row);
        } else {
            m_selected.insert(name);
        }
    }
    return missing;
}

void FolderModel::resolvePending(int page) {
    if (m_pendingRows.isEmpty()) {
        return;
    }
    const int first = page * PageSize;
    bool changed = false;
    for (int row = first; row < first + PageSize; ++row) {
        if (m_pendingRows.remove(row)) {
            const QByteArray name = rawName(row);
            if (!name.isEmpty()) {
                m_selected.insert(name);
            }
            changed = true;
        }
    }
    if (changed) {
        emit selectionChanged();
    }
}

void FolderModel::select(int row) {
    m_selected.clear();
    m_pendingRows.clear();
    m_allSelected = false;
    m_anchor = row;
    addNames(row, row);
    selectionTouched();
}

void FolderModel::toggle(int row) {
    const QByteArray name = rawName(row);
    if (name.isEmpty()) {
        return;
    }
    if (m_selected.contains(name)) {
        m_selected.remove(name);
    } else {
        m_selected.insert(name);
    }
    m_anchor = row;
    selectionTouched();
}

void FolderModel::extendTo(int row) {
    if (m_anchor < 0) {
        select(row);
        return;
    }
    m_selected.clear();
    m_pendingRows.clear();
    m_allSelected = false;
    const QList<int> missing = addNames(qMin(m_anchor, row), qMax(m_anchor, row));
    for (const int missingRow : missing) {
        m_pendingRows.insert(missingRow);
        request(missingRow / PageSize);
    }
    selectionTouched();
}

void FolderModel::selectRows(const QList<int> &rows, bool add) {
    if (!add) {
        m_selected.clear();
        m_pendingRows.clear();
        m_allSelected = false;
    }
    for (const int row : rows) {
        addNames(row, row);
    }
    selectionTouched();
}

void FolderModel::selectAll() {
    m_selected.clear();
    m_pendingRows.clear();
    m_allSelected = true;
    selectionTouched();
}

void FolderModel::clearSelection() {
    if (m_selected.isEmpty() && m_pendingRows.isEmpty() && !m_allSelected) {
        return;
    }
    m_selected.clear();
    m_pendingRows.clear();
    m_allSelected = false;
    m_anchor = -1;
    selectionTouched();
}

QStringList FolderModel::selectedPaths() const {
    QStringList paths;
    for (const QByteArray &name : selectedNames()) {
        paths.append(QFile::decodeName(Wire::join(m_folder, name)));
    }
    return paths;
}

QList<QByteArray> FolderModel::selectedNames() const {
    QList<QByteArray> names;
    if (!m_allSelected) {
        names = QList<QByteArray>(m_selected.cbegin(), m_selected.cend());
        std::sort(names.begin(), names.end());
        return names;
    }
    for (int row = 0; row < m_count; ++row) {
        const QByteArray name = rawName(row);
        if (!name.isEmpty() && !m_selected.contains(name)) {
            names.append(name);
        }
    }
    return names;
}

void FolderModel::rename(int row, const QString &name) {
    const QByteArray from = rawName(row);
    if (from.isEmpty() || name.isEmpty() || QFile::encodeName(name) == from) {
        return;
    }
    const QByteArray folder = m_folder;
    m_backend->call(QStringLiteral("ops.rename"), {{"path", Wire::encode(Wire::join(folder, from))}, {"name", name}}, this,
                    [this, folder](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit failed(error.message);
                            return;
                        }
                        if (folder != m_folder) {
                            return;
                        }
                        // The listing hears of the change on its own; the selection moves with it.
                        const QByteArray now = Wire::baseName(Wire::decode(result.toObject().value(QStringLiteral("path"))));
                        m_selected = {now};
                        m_pendingRows.clear();
                        m_allSelected = false;
                        selectionTouched();
                        emit renamed(Wire::display(now));
                    });
}

QStringList FolderModel::leftovers() const {
    QStringList names;
    for (const QByteArray &name : m_leftovers) {
        names.append(Wire::display(name));
    }
    return names;
}

void FolderModel::takeLeftovers(const QJsonValue &names) {
    QList<QByteArray> originals;
    QJsonArray probes;
    for (const QJsonValue &name : names.toArray()) {
        const QByteArray raw = Wire::decode(name);
        // Probes are only swept (the backend removes old empty ones); originals are offered back.
        if (raw.startsWith(".asc-files-probe-")) {
            probes.append(name);
        } else if (!raw.isEmpty()) {
            originals.append(raw);
        }
    }
    if (!probes.isEmpty() && !m_folder.isEmpty()) {
        m_backend->call(QStringLiteral("leftovers.sweep"), {{"folder", Wire::encode(m_folder)}, {"names", probes}});
    }
    if (originals != m_leftovers) {
        m_leftovers = originals;
        emit leftoversChanged();
    }
}
