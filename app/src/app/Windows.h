// SPDX-License-Identifier: MIT
#pragma once

#include <QList>
#include <QObject>
#include <QPointer>
#include <QQuickWindow>

class QQmlEngine;

/// Files' windows. Each has its own tabs, search and preview; the clipboard, the jobs, the
/// places, Recent, Favorites and the Trash are the app's, shared by them all. Operations
/// answer in the window used last (`current`): its notices, clashes and questions show there
/// only. The first window keeps the session (its tabs come back next time); others don't.
class Windows : public QObject {
    Q_OBJECT
    Q_PROPERTY(QQuickWindow *current READ current NOTIFY currentChanged)
    Q_PROPERTY(int count READ count NOTIFY countChanged)
public:
    /// `shared`: the app-wide objects each window is given, by the names Main.qml takes.
    Windows(QQmlEngine *engine, QVariantMap shared, QObject *parent = nullptr);
    /// Deletes the windows still open or waiting to be deleted, while their engine lives.
    ~Windows() override;

    /// Opens a window on `folders` (one tab each), else on the start folder; the first window brings
    /// back the last session's tabs when it isn't given folders.
    Q_INVOKABLE QQuickWindow *open(const QStringList &folders = {});

    /// Shows `folder` in a new window with `items` (raw paths in it) selected, and with
    /// `properties` their Properties: for another program's "Show in folder".
    void reveal(const QByteArray &folder, const QList<QByteArray> &items, bool properties);

    QQuickWindow *current() const { return m_current; }
    int count() const { return int(m_windows.size()); }

signals:
    void currentChanged();
    void countChanged();
    /// A window could not be made (its QML failed to load).
    void failed();

private:
    void closed(QQuickWindow *window);

    QQmlEngine *m_engine;
    QVariantMap m_shared;
    QList<QQuickWindow *> m_windows;
    /// Closed windows not deleted yet.
    QList<QPointer<QQuickWindow>> m_closed;
    QPointer<QQuickWindow> m_current;
    bool m_sessionTaken = false;
};
