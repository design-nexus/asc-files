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

    /// Opens a window on `folders` (one tab each), else on Home; the first window opened brings
    /// back the last session's tabs when it isn't given folders.
    Q_INVOKABLE QQuickWindow *open(const QStringList &folders = {});

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
    QPointer<QQuickWindow> m_current;
    bool m_sessionTaken = false;
};
