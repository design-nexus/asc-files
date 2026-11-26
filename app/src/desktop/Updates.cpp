// SPDX-License-Identifier: MIT
#include "Updates.h"

#include "../settings/AppSettings.h"

#include <QCoreApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QRegularExpression>

namespace {

const QString Releases = QStringLiteral("https://api.github.com/repos/design-nexus/asc-files/releases?per_page=20");

struct Parsed {
    QList<int> numbers;
    QString pre;
};

Parsed parse(QString version) {
    if (version.startsWith(u'v') || version.startsWith(u'V')) {
        version.remove(0, 1);
    }
    const qsizetype dash = version.indexOf(u'-');
    Parsed parsed{{}, dash >= 0 ? version.mid(dash + 1) : QString()};
    for (const QString &part : version.left(dash >= 0 ? dash : version.size()).split(u'.')) {
        parsed.numbers.append(part.toInt());
    }
    while (parsed.numbers.size() < 3) {
        parsed.numbers.append(0);
    }
    return parsed;
}

/// The version running.
QString current() { return AppSettings::shared().about().value(QStringLiteral("version")).toString(); }

} // namespace

Updates::Updates(QObject *parent) : QObject(parent) {}

int Updates::compareVersions(const QString &a, const QString &b) {
    const Parsed left = parse(a);
    const Parsed right = parse(b);
    for (qsizetype i = 0; i < std::max(left.numbers.size(), right.numbers.size()); ++i) {
        const int x = left.numbers.value(i);
        const int y = right.numbers.value(i);
        if (x != y) {
            return x < y ? -1 : 1;
        }
    }
    if (left.pre == right.pre) {
        return 0;
    }
    // A release is newer than its pre-releases; pre-releases compare as written (beta.2 > beta.1).
    if (left.pre.isEmpty()) {
        return 1;
    }
    if (right.pre.isEmpty()) {
        return -1;
    }
    const QStringList x = left.pre.split(u'.');
    const QStringList y = right.pre.split(u'.');
    for (qsizetype i = 0; i < std::min(x.size(), y.size()); ++i) {
        bool xNumber = false;
        bool yNumber = false;
        const int xn = x[i].toInt(&xNumber);
        const int yn = y[i].toInt(&yNumber);
        const int order = xNumber && yNumber ? (xn < yn ? -1 : xn > yn ? 1 : 0) : x[i].compare(y[i]);
        if (order != 0) {
            return order < 0 ? -1 : 1;
        }
    }
    return x.size() < y.size() ? -1 : x.size() > y.size() ? 1 : 0;
}

QVariantMap Updates::newest(const QJsonArray &releases, const QString &current, const QString &channel) {
    QVariantMap best;
    for (const QJsonValue &value : releases) {
        const QJsonObject release = value.toObject();
        const QString tag = release.value("tag_name").toString();
        if (tag.isEmpty() || release.value("draft").toBool() || (release.value("prerelease").toBool() && channel != QLatin1String("beta"))) {
            continue;
        }
        if (compareVersions(tag, current) <= 0 || (!best.isEmpty() && compareVersions(tag, best.value("version").toString()) <= 0)) {
            continue;
        }
        best = {{"version", tag.startsWith(u'v') ? tag.mid(1) : tag}, {"url", release.value("html_url").toString()}};
    }
    return best;
}

bool Updates::due(bool allowed, const QDateTime &last, const QDateTime &now) {
    return allowed && (!last.isValid() || last.secsTo(now) >= DaySeconds || last > now);
}

bool Updates::packaged() const {
    const QString path = QCoreApplication::applicationFilePath();
    return path.startsWith(QLatin1String("/usr/")) || path.startsWith(QLatin1String("/nix/store/"));
}

void Updates::check(bool force) {
    AppSettings &settings = AppSettings::shared();
    if (m_checking || (!force && !due(settings.checkUpdates(), settings.lastUpdateCheck(), QDateTime::currentDateTime()))) {
        return;
    }
    if (!m_network) {
        m_network = new QNetworkAccessManager(this);
    }
    m_checking = true;
    m_status = tr("Checking…");
    emit changed();
    QNetworkRequest request{QUrl(Releases)};
    request.setHeader(QNetworkRequest::UserAgentHeader, QStringLiteral("asc-files/%1").arg(current()));
    request.setRawHeader("Accept", "application/vnd.github+json");
    request.setTransferTimeout(15000);
    QNetworkReply *reply = m_network->get(request);
    connect(reply, &QNetworkReply::finished, this, [this, reply] {
        reply->deleteLater();
        m_checking = false;
        AppSettings &settings = AppSettings::shared();
        if (reply->error() != QNetworkReply::NoError) {
            m_status = tr("Couldn’t check: %1").arg(reply->errorString());
            emit changed();
            return;
        }
        settings.setLastUpdateCheck(QDateTime::currentDateTime());
        const QVariantMap release = newest(QJsonDocument::fromJson(reply->readAll()).array(), current(),
                                         settings.updateChannel());
        m_available = release.value("version").toString();
        m_notesUrl = release.value("url").toString();
        m_status = m_available.isEmpty() ? tr("Files is up to date") : tr("Files %1 is available").arg(m_available);
        emit changed();
        if (!m_available.isEmpty() && m_available != m_announced) {
            m_announced = m_available;
            emit found(m_available);
        }
    });
}
