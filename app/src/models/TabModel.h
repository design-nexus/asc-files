// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QTimer>

class BackendClient;
#include "Browser.h"

/// The window's tabs, each a Browser of its own. There is always at least one; closing the last
/// tab is refused, as closing it is closing the window.
class TabModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int currentIndex READ currentIndex WRITE setCurrentIndex NOTIFY currentChanged)
    Q_PROPERTY(Browser *current READ current NOTIFY currentChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
public:
    enum Role { BrowserRole = Qt::UserRole + 1, TitleRole };
    Q_ENUM(Role)

    explicit TabModel(BackendClient *backend, QObject *parent = nullptr);

    int currentIndex() const { return m_current; }
    void setCurrentIndex(int index);
    Browser *current() const;
    Browser *at(int index) const;

    /// Opens a tab on `path` (raw bytes) after the current one and makes it current.
    Browser *open(const QByteArray &path);
    /// Opens a tab on a path given as text; empty for the current tab's folder.
    Q_INVOKABLE void openTab(const QString &path = {});
    Q_INVOKABLE void closeTab(int index);
    Q_INVOKABLE void moveTab(int from, int to);
    Q_INVOKABLE void next();
    Q_INVOKABLE void previous();

    /// Opens the tabs the last session left, where they were; false when it left fewer than two.
    bool restoreSession();
    /// Whether this window's tabs are the session kept for next time (the first window's are).
    void setKeepsSession(bool keeps) { m_keepsSession = keeps; }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

signals:
    void currentChanged();
    void countChanged();

private:
    /// Keeps where each tab is, a moment after it changes, for the next session.
    void saveSoon();
    void saveSession() const;

    BackendClient *m_backend;
    QTimer m_save;
    bool m_restoring = false;
    bool m_keepsSession = true;
    QList<Browser *> m_tabs;
    int m_current = -1;
};
