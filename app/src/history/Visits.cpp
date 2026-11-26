// SPDX-License-Identifier: MIT
#include "Visits.h"

#include "Terms.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>

Visits::Visits(const QString &file, QObject *parent) : QObject(parent), m_file(file) {
    QFile saved(m_file);
    if (m_file.isEmpty() || !saved.open(QIODevice::ReadOnly)) {
        return;
    }
    for (const QJsonValue &value : QJsonDocument::fromJson(saved.readAll()).array()) {
        const QJsonObject visit = value.toObject();
        const QString path = visit.value("path").toString();
        if (!path.isEmpty()) {
            m_visits.insert(path, {visit.value("count").toInt(1), QDateTime::fromSecsSinceEpoch(visit.value("last").toInteger())});
        }
    }
    // Read back in the order they were visited.
    QList<QString> paths = m_visits.keys();
    std::sort(paths.begin(), paths.end(), [this](const QString &a, const QString &b) { return m_visits[a].last < m_visits[b].last; });
    for (const QString &path : std::as_const(paths)) {
        m_visits[path].order = ++m_order;
    }
}

QString Visits::defaultFile() {
    const QString state = qEnvironmentVariable("XDG_STATE_HOME", QDir::homePath() + QStringLiteral("/.local/state"));
    return state + QStringLiteral("/asc-files/visits.json");
}

void Visits::record(const QString &folder) {
    if (folder.isEmpty()) {
        return;
    }
    Visit &visit = m_visits[folder];
    ++visit.count;
    visit.last = QDateTime::currentDateTime();
    visit.order = ++m_order;
    // The least worth keeping go first.
    if (m_visits.size() > Kept) {
        const QDateTime now = QDateTime::currentDateTime();
        QString weakest;
        double least = 0;
        for (auto it = m_visits.cbegin(); it != m_visits.cend(); ++it) {
            const double score = frecency(it.key(), now);
            if (weakest.isEmpty() || score < least) {
                weakest = it.key();
                least = score;
            }
        }
        m_visits.remove(weakest);
    }
    save();
}

double Visits::frecency(const QString &folder, const QDateTime &now) const {
    const auto it = m_visits.constFind(folder);
    if (it == m_visits.cend()) {
        return 0;
    }
    // As zoxide weighs them: the last hour counts four times, the last day twice, the last week
    // half, older a quarter.
    const qint64 age = it->last.secsTo(now);
    const double weight = age < 3600 ? 4 : age < 86400 ? 2 : age < 604800 ? 0.5 : 0.25;
    return it->count * weight;
}

QHash<QString, double> Visits::frecencies() const {
    const QDateTime now = QDateTime::currentDateTime();
    QHash<QString, double> scores;
    for (auto it = m_visits.cbegin(); it != m_visits.cend(); ++it) {
        scores.insert(it.key(), frecency(it.key(), now));
    }
    return scores;
}

QStringList Visits::jump(const QString &query, const QString &except) const {
    const Terms terms(query);
    const QString typed = query.trimmed().toLower();
    const QDateTime now = QDateTime::currentDateTime();
    struct Ranked {
        QString path;
        int inName;
        bool exact;
        int loose;
        double frecency;
    };
    QList<Ranked> ranked;
    for (auto it = m_visits.cbegin(); it != m_visits.cend(); ++it) {
        if (it.key() == except || !terms.matches(it.key())) {
            continue;
        }
        const QString name = QFileInfo(it.key()).fileName();
        ranked.append({it.key(), terms.inName(name), !typed.isEmpty() && name.toLower() == typed, terms.looseness(name), frecency(it.key(), now)});
    }
    std::sort(ranked.begin(), ranked.end(), [](const Ranked &a, const Ranked &b) {
        if (a.inName != b.inName) {
            return a.inName > b.inName;
        }
        if (a.exact != b.exact) {
            return a.exact;
        }
        if (a.loose != b.loose) {
            return a.loose < b.loose;
        }
        if (a.frecency != b.frecency) {
            return a.frecency > b.frecency;
        }
        return a.path < b.path;
    });
    QStringList paths;
    for (const Ranked &one : std::as_const(ranked)) {
        paths.append(one.path);
    }
    return paths;
}

QStringList Visits::recent(const QString &query, const QString &except) const {
    const Terms terms(query);
    QList<std::pair<quint64, QString>> found;
    for (auto it = m_visits.cbegin(); it != m_visits.cend(); ++it) {
        if (it.key() != except && terms.matches(it.key())) {
            found.append({it->order, it.key()});
        }
    }
    std::sort(found.begin(), found.end(), [](const auto &a, const auto &b) { return a.first > b.first; });
    QStringList paths;
    for (const auto &[last, path] : std::as_const(found)) {
        paths.append(path);
    }
    return paths;
}

void Visits::save() const {
    if (m_file.isEmpty()) {
        return;
    }
    QDir().mkpath(QFileInfo(m_file).absolutePath());
    QJsonArray visits;
    for (auto it = m_visits.cbegin(); it != m_visits.cend(); ++it) {
        visits.append(QJsonObject{{"path", it.key()}, {"count", it->count}, {"last", it->last.toSecsSinceEpoch()}});
    }
    QSaveFile file(m_file);
    if (file.open(QIODevice::WriteOnly)) {
        file.write(QJsonDocument(visits).toJson(QJsonDocument::Compact));
        file.commit();
    }
}
