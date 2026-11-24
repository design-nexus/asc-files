// SPDX-License-Identifier: MIT
#include "TabModel.h"

#include "Browser.h"

#include <QFile>

TabModel::TabModel(BackendClient *backend, QObject *parent) : QAbstractListModel(parent), m_backend(backend) {}

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
    open(path.isEmpty() && here ? here->rawLocation() : QFile::encodeName(path));
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
