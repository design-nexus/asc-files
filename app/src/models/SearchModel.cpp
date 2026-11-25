// SPDX-License-Identifier: MIT
#include "SearchModel.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"
#include "../settings/AppSettings.h"

#include <QFile>
#include <QJsonArray>

namespace {

ItemListModel::Item itemOf(const QJsonValue &value) {
    const QJsonObject found = value.toObject();
    const qint64 modified = found.value(QStringLiteral("modified")).toInteger();
    return {QFile::decodeName(Wire::decode(found.value(QStringLiteral("path")))), {},
            modified > 0 ? QDateTime::fromSecsSinceEpoch(modified) : QDateTime(), {}, {}};
}

} // namespace

SearchModel::SearchModel(BackendClient *backend, QObject *parent) : ItemListModel(parent), m_backend(backend) {
    m_pause.setSingleShot(true);
    m_pause.setInterval(TypingPauseMs);
    connect(&m_pause, &QTimer::timeout, this, &SearchModel::searchNow);
    connect(m_backend, &BackendClient::notification, this, &SearchModel::notification);
}

void SearchModel::search(const QString &query, const QStringList &roots) {
    m_query = query;
    m_roots = roots;
    emit stateChanged();
    m_pause.start();
}

void SearchModel::addExclusion(const QString &rule) {
    m_backend->call(QStringLiteral("search.exclusion"), {{"rule", rule}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit exclusionRefused(error.message);
                            return;
                        }
                        AppSettings &settings = AppSettings::shared();
                        settings.setSearchExclusions(settings.searchExclusions() << result.toObject().value("rule").toString());
                    });
}

void SearchModel::stop() {
    if (m_search >= 0 && m_running) {
        m_backend->call(QStringLiteral("search.cancel"), {{"search", m_search}});
    }
    m_search = -1;
    if (m_running) {
        m_running = false;
        emit stateChanged();
    }
}

void SearchModel::searchNow() {
    m_pause.stop();
    stop();
    m_found.clear();
    m_truncated = false;
    m_matched = 0;
    setItems({});
    if (m_query.trimmed().isEmpty() || m_roots.isEmpty()) {
        emit stateChanged();
        return;
    }
    QJsonArray roots;
    for (const QString &root : std::as_const(m_roots)) {
        roots.append(Wire::encode(QFile::encodeName(root)));
    }
    m_running = true;
    emit stateChanged();
    const QString query = m_query;
    // Settings › Search: how long and how far, and what is left out.
    const AppSettings &settings = AppSettings::shared();
    const QJsonObject params{{"roots", roots},
                             {"query", query},
                             {"seconds", settings.searchSeconds()},
                             {"entries", settings.searchEntries()},
                             {"exclude", QJsonArray::fromStringList(settings.searchExclusions())}};
    m_backend->call(QStringLiteral("search.start"), params, this,
                    [this, query](const QJsonValue &result, const BackendError &error) {
                        if (query != m_query) {
                            // Typed on meanwhile: this one is not wanted.
                            m_backend->call(QStringLiteral("search.cancel"), {{"search", result.toObject().value("search")}});
                            return;
                        }
                        if (!error.isNull()) {
                            m_running = false;
                            emit stateChanged();
                            return;
                        }
                        m_search = result.toObject().value(QStringLiteral("search")).toInteger();
                    });
}

void SearchModel::notification(const QString &method, const QJsonObject &params) {
    if (!method.startsWith(QLatin1String("search.")) || params.value(QStringLiteral("search")).toInteger() != m_search) {
        return;
    }
    const QJsonArray items = params.value(QStringLiteral("items")).toArray();
    if (method == QLatin1String("search.found")) {
        for (const QJsonValue &item : items) {
            m_found.append(itemOf(item));
        }
        setItems(m_found);
    } else if (method == QLatin1String("search.finished")) {
        // The best, ranked, in place of what streamed in.
        m_found.clear();
        for (const QJsonValue &item : items) {
            m_found.append(itemOf(item));
        }
        setItems(m_found);
        m_running = false;
        m_truncated = params.value(QStringLiteral("truncated")).toBool();
        m_matched = params.value(QStringLiteral("matched")).toInt();
        emit stateChanged();
    }
}
