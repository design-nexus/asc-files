// SPDX-License-Identifier: MIT
#include "Properties.h"

#include "../backend/BackendClient.h"
#include "../desktop/DesktopApps.h"
#include "../settings/AppSettings.h"

#include <QFileInfo>
#include <QImageReader>
#include <QJsonObject>
#include <QLocale>
#include <QMimeDatabase>
#include <unistd.h>

Properties::Properties(BackendClient *backend, DesktopApps *apps, QObject *parent)
    : QObject(parent), m_backend(backend), m_apps(apps) {
    connect(m_backend, &BackendClient::notification, this, &Properties::notification);
}

Properties::~Properties() { stopMeasuring(); }

void Properties::setPath(const QString &path) {
    if (path == m_path) {
        return;
    }
    stopMeasuring();
    m_path = path;
    reload();
    if (m_isFolder && !isLink()) {
        measure();
    }
}

void Properties::reload() {
    if (m_path.isEmpty()) {
        // Nothing shown.
        m_exists = m_isFolder = m_canChange = false;
        m_linkTarget.clear();
        m_kind.clear();
        m_opensWith.clear();
        m_dimensions.clear();
        m_owner.clear();
        m_group.clear();
        m_size = 0;
        m_bits = 0;
        m_modified = m_accessed = {};
        m_totals.clear();
        emit changed();
        emit measured();
        return;
    }
    const QFileInfo info(m_path);
    m_exists = info.exists() || info.isSymLink();
    m_isFolder = info.isDir() && !info.isSymLink();
    m_linkTarget = info.isSymLink() ? info.symLinkTarget() : QString();
    m_size = info.isFile() && !info.isSymLink() ? info.size() : 0;
    m_modified = info.lastModified();
    m_accessed = info.lastRead();
    m_owner = info.owner();
    m_group = info.group();
    const QFile::Permissions permissions = info.permissions();
    const std::pair<QFile::Permission, int> map[] = {
        {QFile::ReadOwner, 0400}, {QFile::WriteOwner, 0200}, {QFile::ExeOwner, 0100},
        {QFile::ReadGroup, 0040}, {QFile::WriteGroup, 0020}, {QFile::ExeGroup, 0010},
        {QFile::ReadOther, 0004}, {QFile::WriteOther, 0002}, {QFile::ExeOther, 0001},
    };
    m_bits = 0;
    for (const auto &[permission, bit] : map) {
        if (permissions & permission) {
            m_bits |= bit;
        }
    }
    m_canChange = m_exists && !info.isSymLink() && (geteuid() == 0 || info.ownerId() == geteuid());
    const QMimeDatabase database;
    const QMimeType type = m_isFolder ? database.mimeTypeForName(QStringLiteral("inode/directory")) : database.mimeTypeForFile(info);
    m_kind = info.isSymLink() ? tr("Link") : type.comment();
    m_opensWith.clear();
    if (!m_isFolder && m_exists) {
        const QVariantList apps = m_apps ? m_apps->appsFor({m_path}, 1) : QVariantList();
        const QVariantMap first = apps.isEmpty() ? QVariantMap() : apps.first().toMap();
        m_opensWith = first.value(QStringLiteral("isDefault")).toBool() ? first.value(QStringLiteral("name")).toString() : tr("No program set");
    }
    m_dimensions.clear();
    if (type.name().startsWith(QLatin1String("image/"))) {
        const QSize size = QImageReader(m_path).size();
        if (size.isValid()) {
            m_dimensions = tr("%1 × %2 pixels").arg(size.width()).arg(size.height());
        }
    }
    emit changed();
    emit measured();
}

void Properties::setBit(int bit, bool on) {
    if (!m_canChange || bit <= 0 || bit > 0777) {
        return;
    }
    apply(on ? (m_bits | bit) : (m_bits & ~bit));
}

void Properties::setRunnable(bool runnable) {
    if (m_canChange) {
        apply(runnable ? m_bits | ((m_bits & 0444) >> 2) : m_bits & ~0111);
    }
}

void Properties::apply(int bits) {
    m_backend->call(QStringLiteral("ops.chmod"), {{"path", m_path}, {"bits", bits}}, this,
                    [this](const QJsonValue &, const BackendError &error) {
                        if (!error.isNull()) {
                            emit failed(error.message);
                        }
                        // The switches show what the item has now, changed or not.
                        reload();
                    });
}

void Properties::measure() {
    m_totals.clear();
    m_measure = 0;
    const quint64 call = ++m_measureCall;
    m_backend->call(QStringLiteral("measure.start"), {{"path", m_path}}, this,
                    [this, call](const QJsonValue &result, const BackendError &error) {
                        const qint64 id = result.toObject().value(QStringLiteral("measure")).toInteger(-1);
                        if (call != m_measureCall) {
                            // Another item meanwhile: this one isn't wanted.
                            if (id >= 0) m_backend->call(QStringLiteral("measure.cancel"), {{"measure", id}});
                            return;
                        }
                        m_measure = error.isNull() ? id : -1;
                        emit measured();
                    });
    emit measured();
}

void Properties::stopMeasuring() {
    ++m_measureCall;
    if (m_measure > 0) {
        m_backend->call(QStringLiteral("measure.cancel"), {{"measure", m_measure}});
    }
    m_measure = -1;
}

void Properties::notification(const QString &method, const QJsonObject &params) {
    if (!method.startsWith(QLatin1String("measure.")) || m_measure <= 0 ||
        params.value(QStringLiteral("measure")).toInteger() != m_measure) {
        return;
    }
    m_totals = params.toVariantMap();
    if (method == QLatin1String("measure.finished")) {
        m_measure = -1;
    }
    emit measured();
}

QString Properties::name() const {
    const QString name = QFileInfo(m_path).fileName();
    return name.isEmpty() ? m_path : name;
}

QString Properties::location() const { return QFileInfo(m_path).path(); }

QString Properties::sizeText() const {
    const QLocale locale;
    if (!m_isFolder) {
        return isLink() ? QString() : tr("%1 (%2 bytes)").arg(locale.formattedDataSize(m_size, 1, QLocale::DataSizeSIFormat), locale.toString(m_size));
    }
    if (m_totals.contains(QStringLiteral("error"))) {
        return tr("Unavailable");
    }
    if (m_totals.isEmpty()) {
        return tr("Calculating…");
    }
    // At least this much, while it is counted or when it couldn't all be.
    const QString prefix = measuring() || !measureWarning().isEmpty() ? QStringLiteral("≥ ") : QString();
    return prefix + locale.formattedDataSize(m_totals.value(QStringLiteral("size")).toLongLong(), 1, QLocale::DataSizeSIFormat);
}

QString Properties::containsText() const {
    if (!m_isFolder || m_totals.isEmpty() || m_totals.contains(QStringLiteral("error"))) {
        return m_totals.contains(QStringLiteral("error")) ? tr("Unavailable") : QString();
    }
    const QString prefix = measuring() || !measureWarning().isEmpty() ? QStringLiteral("≥ ") : QString();
    return prefix + tr("%n files", "", m_totals.value(QStringLiteral("files")).toInt()) + QStringLiteral(", ") + prefix +
           tr("%n folders", "", m_totals.value(QStringLiteral("folders")).toInt());
}

QString Properties::measureWarning() const {
    QStringList reasons;
    if (m_totals.value(QStringLiteral("unreadable")).toBool()) {
        reasons.append(tr("Some folders or entries couldn't be read."));
    }
    if (m_totals.value(QStringLiteral("timedOut")).toBool()) {
        reasons.append(tr("The five-minute calculation limit was reached."));
    }
    if (m_totals.value(QStringLiteral("depthLimited")).toBool()) {
        reasons.append(tr("Some folders exceeded the 64-level nesting limit."));
    }
    return reasons.isEmpty() ? QString() : tr("Totals are incomplete.") + u'\n' + reasons.join(u'\n');
}

QString Properties::modifiedText() const {
    return m_modified.isValid() ? AppSettings::fullDate(m_modified) : QString();
}

QString Properties::accessedText() const {
    return m_accessed.isValid() ? AppSettings::fullDate(m_accessed) : QString();
}

bool Properties::hidden() const { return name().startsWith(u'.'); }

QString Properties::symbolic(int bits) {
    QString text;
    for (int shift : {6, 3, 0}) {
        text += bits & (04 << shift) ? u'r' : u'-';
        text += bits & (02 << shift) ? u'w' : u'-';
        text += bits & (01 << shift) ? u'x' : u'-';
    }
    return text;
}

QString Properties::modeText() const {
    return QStringLiteral("%1 (%2)").arg(symbolic(m_bits), QString::number(m_bits, 8).rightJustified(3, u'0'));
}
