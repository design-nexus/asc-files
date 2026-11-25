// SPDX-License-Identifier: MIT
#pragma once

#include <QDateTime>
#include <QObject>
#include <QVariantMap>

class BackendClient;
class DesktopApps;

/// What Properties shows of one item, as legacy Files showed it: its name and type, where it
/// is, its size (a folder's measured as you watch, by the backend), when it changed, what opens
/// it, whether it is hidden, a picture's size, and who may read, write and run it, which can be
/// changed (each change a chmod by the backend; the switches follow what the item has after).
class Properties : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY changed)
    Q_PROPERTY(QString name READ name NOTIFY changed)
    Q_PROPERTY(QString kind READ kind NOTIFY changed)
    Q_PROPERTY(QString location READ location NOTIFY changed)
    Q_PROPERTY(bool isFolder READ isFolder NOTIFY changed)
    Q_PROPERTY(bool isLink READ isLink NOTIFY changed)
    Q_PROPERTY(QString linkTarget READ linkTarget NOTIFY changed)
    Q_PROPERTY(QString sizeText READ sizeText NOTIFY measured)
    /// A folder's files and folders ("12 files, 3 folders").
    Q_PROPERTY(QString containsText READ containsText NOTIFY measured)
    Q_PROPERTY(bool measuring READ measuring NOTIFY measured)
    /// Why a folder's totals are incomplete; empty when they aren't.
    Q_PROPERTY(QString measureWarning READ measureWarning NOTIFY measured)
    Q_PROPERTY(QString modifiedText READ modifiedText NOTIFY changed)
    Q_PROPERTY(QString accessedText READ accessedText NOTIFY changed)
    Q_PROPERTY(QString opensWith READ opensWith NOTIFY changed)
    Q_PROPERTY(bool hidden READ hidden NOTIFY changed)
    /// A picture's width × height, read from its header; empty for others.
    Q_PROPERTY(QString dimensions READ dimensions NOTIFY changed)
    Q_PROPERTY(QString owner READ owner NOTIFY changed)
    Q_PROPERTY(QString group READ group NOTIFY changed)
    /// The read, write and run bits for owner, group and others (`0o755`).
    Q_PROPERTY(int bits READ bits NOTIFY changed)
    /// As `ls -l` writes them (`rwxr-xr-x`), with the octal (`755`).
    Q_PROPERTY(QString modeText READ modeText NOTIFY changed)
    /// Whether this user may change them (the owner, or root), and the item is not a link.
    Q_PROPERTY(bool canChange READ canChange NOTIFY changed)
public:
    Properties(BackendClient *backend, DesktopApps *apps, QObject *parent = nullptr);
    ~Properties() override;

    QString path() const { return m_path; }
    void setPath(const QString &path);
    /// Reads the item again (after a change).
    Q_INVOKABLE void reload();
    /// Turns one bit (`0o400` … `0o001`) on or off.
    Q_INVOKABLE void setBit(int bit, bool on);
    /// Legacy's "Allow running as a program": run for whoever may read it, or for no one.
    Q_INVOKABLE void setRunnable(bool runnable);

    QString name() const;
    QString kind() const { return m_kind; }
    QString location() const;
    bool isFolder() const { return m_isFolder; }
    bool isLink() const { return !m_linkTarget.isEmpty(); }
    QString linkTarget() const { return m_linkTarget; }
    QString sizeText() const;
    QString containsText() const;
    bool measuring() const { return m_measure >= 0; }
    QString measureWarning() const;
    QString modifiedText() const;
    QString accessedText() const;
    QString opensWith() const { return m_opensWith; }
    bool hidden() const;
    QString dimensions() const { return m_dimensions; }
    QString owner() const { return m_owner; }
    QString group() const { return m_group; }
    int bits() const { return m_bits; }
    QString modeText() const;
    bool canChange() const { return m_canChange; }

    /// `rwxr-x---` for `bits`.
    static QString symbolic(int bits);

signals:
    void changed();
    void measured();
    /// A change was refused; `message` says why.
    void failed(const QString &message);

private:
    void apply(int bits);
    void measure();
    void stopMeasuring();
    void notification(const QString &method, const QJsonObject &params);

    BackendClient *m_backend;
    DesktopApps *m_apps;
    QString m_path;
    QString m_kind;
    QString m_linkTarget;
    bool m_isFolder = false;
    bool m_exists = false;
    qint64 m_size = 0;
    QDateTime m_modified;
    QDateTime m_accessed;
    QString m_opensWith;
    QString m_dimensions;
    QString m_owner;
    QString m_group;
    int m_bits = 0;
    bool m_canChange = false;
    // A folder's measurement: its id while it runs (-1 when not), and the totals.
    qint64 m_measure = -1;
    quint64 m_measureCall = 0;
    QVariantMap m_totals;
};
