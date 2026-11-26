// SPDX-License-Identifier: MIT
#pragma once

#include <QDateTime>
#include <QJsonArray>
#include <QObject>
#include <QVariantMap>

class QNetworkAccessManager;

/// Whether a newer Files is out: the repository's releases on GitHub, read at most once a day when
/// Settings allows it (on by default, as legacy did), or when asked. Files never replaces itself:
/// an installed package is updated by the system's package manager.
class Updates : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool checking READ checking NOTIFY changed)
    /// The newer version, or empty.
    Q_PROPERTY(QString available READ available NOTIFY changed)
    /// Its release page.
    Q_PROPERTY(QString notesUrl READ notesUrl NOTIFY changed)
    /// What the last check found, in words.
    Q_PROPERTY(QString status READ status NOTIFY changed)
    /// Files came from a package (under /usr or the Nix store): the package manager updates it.
    Q_PROPERTY(bool packaged READ packaged CONSTANT)
public:
    static constexpr int DaySeconds = 24 * 60 * 60;
    explicit Updates(QObject *parent = nullptr);

    /// -1, 0 or 1 as version `a` is older, the same or newer than `b` (`v1.2.3`, `1.2.3-beta.1`;
    /// a pre-release is older than its release).
    static int compareVersions(const QString &a, const QString &b);
    /// The newest release in GitHub's `releases` answer newer than `current` on `channel`
    /// (`stable`, or `beta` for pre-releases too): `{version, url}`, or empty.
    static QVariantMap newest(const QJsonArray &releases, const QString &current, const QString &channel);
    /// A check is due: allowed, and none in the last day.
    static bool due(bool allowed, const QDateTime &last, const QDateTime &now);

    /// Checks now (`force`), or if one is due.
    Q_INVOKABLE void check(bool force = false);

    bool checking() const { return m_checking; }
    QString available() const { return m_available; }
    QString notesUrl() const { return m_notesUrl; }
    QString status() const { return m_status; }
    bool packaged() const;

signals:
    void changed();
    /// A newer version was found (once per version per session).
    void found(const QString &version);

private:
    QNetworkAccessManager *m_network = nullptr;
    bool m_checking = false;
    QString m_available;
    QString m_notesUrl;
    QString m_status;
    QString m_announced;
};
