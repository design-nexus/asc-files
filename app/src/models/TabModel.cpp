// SPDX-License-Identifier: MIT
#include "TabModel.h"

#include "Browser.h"
#include "../settings/AppSettings.h"

#include <QFile>
#include <QSettings>

namespace {

QSettings settings() {
    return QSettings(QSettings::IniFormat, QSettings::UserScope, QStringLiteral("asc-files"), QStringLiteral("asc-files"));
}

} // namespace

TabModel::TabModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {
    m_save.setSingleShot(true);
    m_save.setInterval(500);
    connect(&m_save, &QTimer::timeout, this, &TabModel::saveSession);
    connect(this, &TabModel::currentChanged, this, &TabModel::saveSoon);
    connect(this, &TabModel::countChanged, this, &TabModel::saveSoon);
}

void TabModel::saveSoon() {
    if (!m_restoring && m_keepsSession) {
        m_save.start();
    }
}

void TabModel::saveSession() const {
    QStringList saved;
    for (const Browser *browser : m_tabs) {
        saved.append(browser->saved());
    }
    QSettings session = settings();
    session.setValue(QStringLiteral("session/tabs"), saved);
    session.setValue(QStringLiteral("session/current"), m_current);
}

bool TabModel::restoreSession() {
    if (!AppSettings::shared().restoreTabs()) {
        return false;
    }
    const QSettings session = settings();
    const QStringList saved = session.value(QStringLiteral("session/tabs")).toStringList();
    // As legacy Files: a single tab is not brought back; Files starts at Home.
    if (saved.size() < 2) {
        return false;
    }
    m_restoring = true;
    for (const QString &place : saved) {
        open({})->restore(place);
    }
    m_restoring = false;
    setCurrentIndex(qBound(0, session.value(QStringLiteral("session/current")).toInt(), int(m_tabs.size()) - 1));
    return true;
}

void TabModel::setCurrentIndex(int index) {
    if (index >= 0 && index < m_tabs.size() && index != m_current) {
        m_current = index;
        emit currentChanged();
    }
}

Browser *TabModel::current() const { return at(m_current); }

Browser *TabModel::at(int index) const { return index >= 0 && index < m_tabs.size() ? m_tabs.at(index) : nullptr; }

Browser *TabModel::open(const QByteArray &path) {
    auto *browser = new Browser(m_backend, this);
    const int row = m_current + 1;
    connect(browser, &Browser::locationChanged, this, [this, browser] {
        const int at = int(m_tabs.indexOf(browser));
        if (at >= 0) {
            emit dataChanged(index(at), index(at), {TitleRole});
        }
        saveSoon();
    });
    beginInsertRows({}, row, row);
    m_tabs.insert(row, browser);
    endInsertRows();
    emit countChanged();
    browser->go(path);
    m_current = row;
    emit currentChanged();
    return browser;
}

void TabModel::openTab(const QString &path) {
    const Browser *here = current();
    if (path.isEmpty() && here) {
        // A new tab starts where this one is, folder or page.
        open({})->restore(here->saved());
        return;
    }
    open(QFile::encodeName(path));
}

void TabModel::duplicateTab(int index) {
    if (index < 0 || index >= m_tabs.size()) {
        return;
    }
    setCurrentIndex(index);
    open({})->restore(m_tabs.at(index)->saved());
}

void TabModel::closeOtherTabs(int index) {
    if (index < 0 || index >= m_tabs.size()) {
        return;
    }
    closeTabsAfter(index);
    while (m_tabs.size() > 1) {
        closeTab(0);
    }
}

void TabModel::closeTabsAfter(int index) {
    while (index >= 0 && index + 1 < m_tabs.size()) {
        closeTab(int(m_tabs.size()) - 1);
    }
}

QString TabModel::folderAt(int index) const {
    if (index < 0 || index >= m_tabs.size() || m_tabs.at(index)->page() != Browser::Folder) {
        return {};
    }
    return m_tabs.at(index)->location();
}

void TabModel::closeTab(int index) {
    if (m_tabs.size() <= 1 || index < 0 || index >= m_tabs.size()) {
        return;
    }
    beginRemoveRows({}, index, index);
    Browser *closed = m_tabs.takeAt(index);
    endRemoveRows();
    emit countChanged();
    if (index < m_current || m_current >= m_tabs.size()) {
        --m_current;
    }
    emit currentChanged();
    closed->deleteLater();
}

void TabModel::moveTab(int from, int to) {
    if (from == to || from < 0 || to < 0 || from >= m_tabs.size() || to >= m_tabs.size()) {
        return;
    }
    Browser *wasCurrent = current();
    beginMoveRows({}, from, from, {}, to > from ? to + 1 : to);
    m_tabs.move(from, to);
    endMoveRows();
    m_current = int(m_tabs.indexOf(wasCurrent));
    emit currentChanged();
}

void TabModel::next() { setCurrentIndex((m_current + 1) % int(m_tabs.size())); }

void TabModel::previous() { setCurrentIndex((m_current + int(m_tabs.size()) - 1) % int(m_tabs.size())); }

int TabModel::rowCount(const QModelIndex &parent) const { return parent.isValid() ? 0 : int(m_tabs.size()); }

QVariant TabModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= m_tabs.size()) {
        return {};
    }
    Browser *browser = m_tabs.at(index.row());
    switch (role) {
    case BrowserRole:
        return QVariant::fromValue(browser);
    case TitleRole:
        return browser->title();
    default:
        return {};
    }
}

QHash<int, QByteArray> TabModel::roleNames() const { return {{BrowserRole, "browser"}, {TitleRole, "title"}}; }
