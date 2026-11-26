// SPDX-License-Identifier: MIT
#pragma once

#include <QDateTime>
#include <QHash>
#include <QObject>
#include <QStringList>

/// The folders Files has opened, how often and when (`~/.local/state/asc-files/visits.json`): 10xer's
/// z (by name, then often and lately) and Z (latest first), and the folder picker's ranking.
class Visits : public QObject {
    Q_OBJECT
public:
    struct Visit {
        int count = 0;
        QDateTime last;
        /// Which came later, where the clock can't tell.
        quint64 order = 0;
    };
    static constexpr int Kept = 1000;

    /// `file` is where they are kept; empty for none (tests).
    explicit Visits(const QString &file = defaultFile(), QObject *parent = nullptr);
    static QString defaultFile();

    Q_INVOKABLE void record(const QString &folder);
    /// The folders matching `query` (search's terms over the whole path), those whose names match
    /// more terms first, a name typed exactly first among them, then by frecency; `except` left out.
    Q_INVOKABLE QStringList jump(const QString &query, const QString &except = {}) const;
    /// The folders matching `query`, the latest visited first.
    Q_INVOKABLE QStringList recent(const QString &query, const QString &except = {}) const;
    /// How often and how lately `folder` was opened, as one number (0 for never).
    double frecency(const QString &folder, const QDateTime &now = QDateTime::currentDateTime()) const;
    /// Every folder's frecency.
    QHash<QString, double> frecencies() const;

private:
    void save() const;
    QString m_file;
    QHash<QString, Visit> m_visits;
    quint64 m_order = 0;
};
