// SPDX-License-Identifier: MIT
#pragma once

#include <QObject>

class BackendClient;

/// Whether Files is the desktop's file chooser and file manager, and turning each on or off
/// (Settings › Integrations), through the backend.
class Integrations : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool chooser READ chooser NOTIFY changed)
    Q_PROPERTY(bool fileManager READ fileManager NOTIFY changed)
    Q_PROPERTY(bool busy READ busy NOTIFY changed)
public:
    explicit Integrations(BackendClient *backend, QObject *parent = nullptr);

    bool chooser() const { return m_chooser; }
    bool fileManager() const { return m_fileManager; }
    bool busy() const { return m_busy; }

    Q_INVOKABLE void refresh();
    Q_INVOKABLE void setChooser(bool enable);
    Q_INVOKABLE void setFileManager(bool enable);

signals:
    void changed();
    /// Why a change didn't happen, or what it did that the user should know.
    void notice(const QString &message);

private:
    void change(const QString &method, bool enable);
    BackendClient *m_backend;
    bool m_chooser = false;
    bool m_fileManager = false;
    bool m_busy = false;
};
