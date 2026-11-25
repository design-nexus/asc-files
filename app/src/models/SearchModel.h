// SPDX-License-Identifier: MIT
#pragma once

#include "ItemListModel.h"

#include <QStringList>
#include <QTimer>

class BackendClient;

/// A search's results (`search.*`): what the backend finds, shown as it streams in, then its
/// best, ranked. Typing starts a new search a moment after the last key; the last one stops.
class SearchModel : public ItemListModel {
    Q_OBJECT
    Q_PROPERTY(QString query READ query NOTIFY stateChanged)
    Q_PROPERTY(bool running READ running NOTIFY stateChanged)
    Q_PROPERTY(bool truncated READ truncated NOTIFY stateChanged)
    Q_PROPERTY(int matched READ matched NOTIFY stateChanged)
    /// Where it searches: Home and the mounted drives, or one folder.
    Q_PROPERTY(QStringList roots READ roots NOTIFY stateChanged)
public:
    static constexpr int TypingPauseMs = 200;

    explicit SearchModel(BackendClient *backend, QObject *parent = nullptr);

    /// Searches `roots` for `query`, once typing pauses; an empty query clears the results.
    Q_INVOKABLE void search(const QString &query, const QStringList &roots);
    /// Searches now, without waiting for a pause.
    Q_INVOKABLE void searchNow();
    Q_INVOKABLE void stop();
    /// Adds a rule to Settings' search exclusions, as the backend keeps it; one that can't be is
    /// refused, saying why.
    Q_INVOKABLE void addExclusion(const QString &rule);

    QString query() const { return m_query; }
    bool running() const { return m_running; }
    bool truncated() const { return m_truncated; }
    int matched() const { return m_matched; }
    QStringList roots() const { return m_roots; }

signals:
    void stateChanged();
    void exclusionRefused(const QString &why);

private:
    void notification(const QString &method, const QJsonObject &params);

    BackendClient *m_backend;
    QTimer m_pause;
    QString m_query;
    QStringList m_roots;
    qint64 m_search = -1;
    bool m_running = false;
    bool m_truncated = false;
    int m_matched = 0;
    QList<Item> m_found;
};
