// SPDX-License-Identifier: MIT
#include "Browser.h"

#include "../backend/Wire.h"
#include "FolderModel.h"
#include "FolderViews.h"
#include "../settings/AppSettings.h"

#include <QDir>
#include <QFile>
#include <QHash>
#include <QVariantMap>

namespace {

/// History kept in each direction.
constexpr int MaxHistory = 100;

QByteArray homePath() { return QFile::encodeName(QDir::homePath()); }

bool isWithin(const QByteArray &path, const QByteArray &folder) {
    return path == folder || (path.startsWith(folder) && path.at(folder.size()) == '/');
}

} // namespace

Browser::Browser(BackendClient *backend, QObject *parent)
    : QObject(parent), m_folders(new FolderModel(backend, this)), m_columns(new ColumnsModel(backend, this)), m_backend(backend) {
    connect(this, &Browser::locationChanged, this, &Browser::updateColumns);
    connect(this, &Browser::viewChanged, this, &Browser::updateColumns);
    // The folder model may move on its own (its folder was deleted): follow it.
    connect(m_folders, &FolderModel::folderChanged, this, [this] {
        if (m_here.page == Folder) {
            m_here.path = m_folders->rawFolder();
        }
        emit locationChanged();
    });
}

QByteArray Browser::rawLocation() const { return m_here.page == Folder ? m_here.path : QByteArray(); }

QString Browser::location() const { return m_here.page == Folder ? Wire::display(m_here.path) : QString(); }

QString Browser::title() const {
    switch (m_here.page) {
    case Overview:
        return tr("Overview");
    case Recent:
        return tr("Recent");
    case Favorites:
        return tr("Favorites");
    case Trash:
        return tr("Trash");
    case Search:
        return tr("Search");
    case Folder:
        break;
    }
    const QByteArray path = rawLocation();
    if (path == homePath()) {
        return tr("Home");
    }
    return m_folders->folderName();
}

bool Browser::canGoUp() const {
    const QByteArray path = rawLocation();
    return !path.isEmpty() && path != "/";
}

void Browser::go(const QByteArray &path) {
    if (!path.isEmpty()) {
        go(Place{Folder, path});
    }
}

void Browser::goPage(Page page) { go(Place{page, {}}); }

void Browser::go(const Place &place) {
    if (place == m_here) {
        return;
    }
    if (m_here.page != Folder || !m_here.path.isEmpty()) {
        m_back.append(m_here);
        if (m_back.size() > MaxHistory) {
            m_back.removeFirst();
        }
    }
    m_forward.clear();
    show(place);
    emit historyChanged();
}

void Browser::show(const Place &place) {
    m_here = place;
    if (place.page == Folder) {
        // The folder opens as it was last shown there. One never set takes the default sort, and
        // keeps the view the tab is in (a new tab: the default view), so browsing doesn't switch
        // views. When folders don't keep their own, the tab's view and sort go along.
        const QString folder = QFile::decodeName(place.path);
        const bool first = m_folders->rawFolder().isEmpty();
        FolderViews::Choice choice;
        if (AppSettings::shared().rememberFolderViews()) {
            choice = FolderViews::shared().choice(folder);
            if (!FolderViews::shared().has(folder) && !first) {
                choice.view = m_view;
                choice.iconSize = m_iconSize;
            }
        } else if (first) {
            choice = FolderViews::defaults();
        } else {
            choice = {m_view, m_folders->sortKey(), m_folders->sortDescending(), m_iconSize};
        }
        if (m_iconSize != choice.iconSize) {
            m_iconSize = choice.iconSize;
            emit iconSizeChanged();
        }
        m_folders->open(place.path, choice.sortKey, choice.descending);
        if (m_view != choice.view) {
            m_view = choice.view;
            makeSecond();
            emit viewChanged();
        }
    }
    emit locationChanged();
}

void Browser::setView(int view) {
    if (view == m_view || view < FolderViews::List || view > FolderViews::LastView) {
        return;
    }
    m_view = view;
    makeSecond();
    emit viewChanged();
    rememberChoice();
}

void Browser::setIconSize(int size) {
    size = qBound(FolderViews::MinIconSize, size, FolderViews::MaxIconSize);
    if (size == m_iconSize) {
        return;
    }
    m_iconSize = size;
    emit iconSizeChanged();
    rememberChoice();
}

void Browser::zoom(int steps) { setIconSize(m_iconSize + steps * FolderViews::IconSizeStep); }

void Browser::makeSecond() {
    if (m_view == FolderViews::DualPane && !m_second && m_here.page == Folder) {
        m_second = new Browser(m_backend, this);
        m_second->goPath(location());
        emit secondChanged();
    }
}

void Browser::sortBy(const QString &key) {
    const bool descending = key == m_folders->sortKey() ? !m_folders->sortDescending() : key != QLatin1String("name");
    m_folders->setSort(key, descending);
    rememberChoice();
}

void Browser::rememberChoice() {
    if (m_here.page == Folder && !m_here.path.isEmpty() && AppSettings::shared().rememberFolderViews()) {
        FolderViews::shared().remember(QFile::decodeName(m_here.path),
                                       {m_view, m_folders->sortKey(), m_folders->sortDescending(), m_iconSize});
    }
}

QString Browser::saved() const {
    switch (m_here.page) {
    case Overview:
        return QStringLiteral("page:overview");
    case Recent:
        return QStringLiteral("page:recent");
    case Favorites:
        return QStringLiteral("page:favorites");
    case Trash:
        return QStringLiteral("page:trash");
    case Search:
        return QStringLiteral("page:search");
    case Folder:
        break;
    }
    return QFile::decodeName(m_here.path);
}

void Browser::restore(const QString &saved) {
    static const QHash<QString, Page> pages{{QStringLiteral("page:overview"), Overview},
                                            {QStringLiteral("page:recent"), Recent},
                                            {QStringLiteral("page:favorites"), Favorites},
                                            {QStringLiteral("page:trash"), Trash},
                                            {QStringLiteral("page:search"), Search}};
    if (pages.contains(saved)) {
        goPage(pages.value(saved));
    } else if (saved.startsWith(u'/')) {
        goPath(saved);
    }
}

void Browser::goPath(const QString &path) { go(QFile::encodeName(path)); }

void Browser::goRow(int row) {
    const QByteArray name = m_folders->rawName(row);
    if (!name.isEmpty()) {
        go(Wire::join(rawLocation(), name));
    }
}

void Browser::goCrumb(int index) {
    if (m_here.page != Folder) {
        return;
    }
    const QList<QByteArray> paths = crumbPaths();
    if (index >= 0 && index < paths.size()) {
        go(paths.at(index));
    }
}

bool Browser::goTyped(const QString &text) {
    QString typed = text.trimmed();
    if (typed.isEmpty()) {
        return false;
    }
    if (typed == QLatin1String("~") || typed.startsWith(QLatin1String("~/"))) {
        typed = QDir::homePath() + typed.mid(1);
    } else if (!typed.startsWith(u'/')) {
        typed = QDir(m_here.page == Folder ? location() : QDir::homePath()).filePath(typed);
    }
    go(QFile::encodeName(QDir::cleanPath(typed)));
    return true;
}

void Browser::back() {
    if (m_back.isEmpty()) {
        return;
    }
    m_forward.append(m_here);
    show(m_back.takeLast());
    emit historyChanged();
}

void Browser::forward() {
    if (m_forward.isEmpty()) {
        return;
    }
    m_back.append(m_here);
    show(m_forward.takeLast());
    emit historyChanged();
}

void Browser::up() {
    if (canGoUp()) {
        go(Wire::parent(rawLocation()));
    }
}

QList<QByteArray> Browser::crumbPaths() const {
    const QByteArray path = rawLocation();
    if (path.isEmpty()) {
        return {};
    }
    QList<QByteArray> paths;
    const QByteArray home = homePath();
    QByteArray start = "/";
    if (isWithin(path, home)) {
        start = home;
    }
    paths.append(start);
    const QByteArray rest = path.mid(start.size());
    QByteArray walked = start;
    for (const QByteArray &name : rest.split('/')) {
        if (!name.isEmpty()) {
            walked = Wire::join(walked, name);
            paths.append(walked);
        }
    }
    return paths;
}

QVariantList Browser::crumbs() const {
    QVariantList result;
    if (m_here.page != Folder) {
        return {QVariantMap{{QStringLiteral("label"), title()}, {QStringLiteral("home"), false}}};
    }
    const QByteArray home = homePath();
    for (const QByteArray &path : crumbPaths()) {
        const bool isHome = path == home;
        const QString label = isHome ? tr("Home") : path == "/" ? tr("Computer") : Wire::display(Wire::baseName(path));
        result.append(QVariantMap{{QStringLiteral("label"), label}, {QStringLiteral("home"), isHome}, {QStringLiteral("path"), Wire::display(path)}});
    }
    return result;
}

void Browser::updateColumns() {
    // Only while Columns view is chosen: the listings each column holds cost a reader.
    if (m_view == FolderViews::Columns && m_here.page == Folder && !m_here.path.isEmpty()) {
        m_columns->setPath(m_here.path, m_folders);
    }
}

void Browser::goInto(const QByteArray &path, const QByteArray &name) {
    if (!name.isEmpty()) {
        go(Wire::join(path, name));
    }
}

void Browser::goColumnRow(FolderModel *column, int row) {
    if (column) {
        goInto(column->rawFolder(), column->rawName(row));
    }
}
