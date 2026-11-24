// SPDX-License-Identifier: MIT
#include "Browser.h"

#include "../backend/Wire.h"
#include "FolderModel.h"

#include <QDir>
#include <QFile>
#include <QVariantMap>

namespace {

/// History kept in each direction.
constexpr int MaxHistory = 100;

QByteArray homePath() { return QFile::encodeName(QDir::homePath()); }

bool isWithin(const QByteArray &path, const QByteArray &folder) {
    return path == folder || (path.startsWith(folder) && path.at(folder.size()) == '/');
}

} // namespace

Browser::Browser(BackendClient *backend, QObject *parent) : QObject(parent), m_folders(new FolderModel(backend, this)) {
    connect(m_folders, &FolderModel::folderChanged, this, &Browser::locationChanged);
}

QByteArray Browser::rawLocation() const { return m_folders->rawFolder(); }

QString Browser::location() const { return m_folders->folder(); }

QString Browser::title() const {
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
    const QByteArray here = rawLocation();
    if (path.isEmpty() || path == here) {
        return;
    }
    if (!here.isEmpty()) {
        m_back.append(here);
        if (m_back.size() > MaxHistory) {
            m_back.removeFirst();
        }
    }
    m_forward.clear();
    m_folders->open(path);
    emit historyChanged();
}

void Browser::goPath(const QString &path) { go(QFile::encodeName(path)); }

void Browser::goRow(int row) {
    const QByteArray name = m_folders->rawName(row);
    if (!name.isEmpty()) {
        go(Wire::join(rawLocation(), name));
    }
}

void Browser::goCrumb(int index) {
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
        typed = QDir(location()).filePath(typed);
    }
    go(QFile::encodeName(QDir::cleanPath(typed)));
    return true;
}

void Browser::back() {
    if (m_back.isEmpty()) {
        return;
    }
    m_forward.append(rawLocation());
    m_folders->open(m_back.takeLast());
    emit historyChanged();
}

void Browser::forward() {
    if (m_forward.isEmpty()) {
        return;
    }
    m_back.append(rawLocation());
    m_folders->open(m_forward.takeLast());
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
    const QByteArray home = homePath();
    for (const QByteArray &path : crumbPaths()) {
        const bool isHome = path == home;
        const QString label = isHome ? tr("Home") : path == "/" ? tr("Computer") : Wire::display(Wire::baseName(path));
        result.append(QVariantMap{{QStringLiteral("label"), label}, {QStringLiteral("home"), isHome}});
    }
    return result;
}
