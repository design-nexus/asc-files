// SPDX-License-Identifier: MIT
#include "Transfers.h"

#include "../backend/Wire.h"
#include "FolderModel.h"
#include "../ops/Operations.h"
#include "../settings/AppSettings.h"

#include <QFile>
#include <sys/stat.h>

namespace {

/// The device a path is on, or -1.
qint64 deviceOf(const QByteArray &path) {
    struct stat st {};
    return ::lstat(path.constData(), &st) == 0 ? qint64(st.st_dev) : -1;
}

QByteArray localPath(const QUrl &url) { return url.isLocalFile() ? QFile::encodeName(url.toLocalFile()) : QByteArray(); }

} // namespace

Transfers::Transfers(Operations *ops, QObject *parent) : QObject(parent), m_ops(ops) {}

QString Transfers::dragUris(FolderModel *folder, int row) const {
    if (!folder) {
        return {};
    }
    if (!folder->isSelected(row)) {
        folder->select(row);
    }
    QStringList uris;
    for (const QByteArray &name : folder->selectedNames()) {
        uris.append(QString::fromLatin1(QUrl::fromLocalFile(QFile::decodeName(Wire::join(folder->rawFolder(), name))).toEncoded()));
    }
    return uris.join(QStringLiteral("\r\n"));
}

bool Transfers::movesInto(const QList<QUrl> &urls, const QString &target) const {
    const qint64 device = deviceOf(QFile::encodeName(target));
    return !urls.isEmpty() && std::all_of(urls.cbegin(), urls.cend(), [&](const QUrl &url) {
        const QByteArray path = localPath(url);
        return !path.isEmpty() && deviceOf(path) == device;
    });
}

int Transfers::drop(const QList<QUrl> &urls, const QString &target) { return dropRaw(urls, QFile::encodeName(target)); }

int Transfers::dropInto(const QList<QUrl> &urls, FolderModel *folder, int row) {
    const QByteArray name = folder ? folder->rawName(row) : QByteArray();
    return !name.isEmpty() && folder->isFolder(row) ? dropRaw(urls, Wire::join(folder->rawFolder(), name)) : Refused;
}

int Transfers::dropHere(const QList<QUrl> &urls, FolderModel *folder) {
    return folder && !folder->rawFolder().isEmpty() ? dropRaw(urls, folder->rawFolder()) : Refused;
}

int Transfers::dropRaw(const QList<QUrl> &urls, const QByteArray &into) {
    QList<QByteArray> sources;
    for (const QUrl &url : urls) {
        const QByteArray path = localPath(url);
        // Already here, or the folder dropped into itself or into one below it.
        if (path.isEmpty() || Wire::parent(path) == into || into == path || into.startsWith(path + '/')) {
            continue;
        }
        sources.append(path);
    }
    if (sources.isEmpty()) {
        return Refused;
    }
    if (movesInto(urls, QFile::decodeName(into))) {
        m_ops->transfer(sources, into, true);
        return Started;
    }
    const QString choice = AppSettings::shared().crossDriveDrop();
    if (choice == QLatin1String("ask")) {
        m_askedSources = sources;
        m_askedInto = into;
        return Asking;
    }
    m_ops->transfer(sources, into, choice == QLatin1String("move"));
    return Started;
}

void Transfers::answer(bool copy, bool move) {
    const QList<QByteArray> sources = std::exchange(m_askedSources, {});
    const QByteArray into = std::exchange(m_askedInto, {});
    if (!sources.isEmpty() && (copy || move)) {
        m_ops->transfer(sources, into, move);
    }
}
