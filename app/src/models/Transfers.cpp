// SPDX-License-Identifier: MIT
#include "Transfers.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"
#include "FolderModel.h"
#include "JobModel.h"

#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <sys/stat.h>

namespace {

/// The device a path is on, or -1.
qint64 deviceOf(const QByteArray &path) {
    struct stat st {};
    return ::lstat(path.constData(), &st) == 0 ? qint64(st.st_dev) : -1;
}

QByteArray localPath(const QUrl &url) { return url.isLocalFile() ? QFile::encodeName(url.toLocalFile()) : QByteArray(); }

} // namespace

Transfers::Transfers(JobModel *jobs, QObject *parent) : QObject(parent), m_jobs(jobs) {}

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

bool Transfers::drop(const QList<QUrl> &urls, const QString &target) { return dropRaw(urls, QFile::encodeName(target)); }

bool Transfers::dropInto(const QList<QUrl> &urls, FolderModel *folder, int row) {
    const QByteArray name = folder ? folder->rawName(row) : QByteArray();
    return !name.isEmpty() && folder->isFolder(row) && dropRaw(urls, Wire::join(folder->rawFolder(), name));
}

bool Transfers::dropHere(const QList<QUrl> &urls, FolderModel *folder) {
    return folder && !folder->rawFolder().isEmpty() && dropRaw(urls, folder->rawFolder());
}

bool Transfers::dropRaw(const QList<QUrl> &urls, const QByteArray &into) {
    const QString target = Wire::display(into);
    const bool move = movesInto(urls, QFile::decodeName(into));
    QJsonArray items;
    for (const QUrl &url : urls) {
        const QByteArray path = localPath(url);
        // Already here, or the folder dropped into itself or into one below it.
        if (path.isEmpty() || Wire::parent(path) == into || into == path || into.startsWith(path + '/')) {
            continue;
        }
        items.append(QJsonObject{{"source", Wire::encode(path)}, {"action", move ? "move" : "copy"}, {"conflict", "keepBoth"}});
    }
    if (items.isEmpty()) {
        return false;
    }
    const QString name = QFileInfo(target).fileName().isEmpty() ? target : QFileInfo(target).fileName();
    const QString message = move ? tr("Moving %n items to %1", "", int(items.size())).arg(name)
                                 : tr("Copying %n items to %1", "", int(items.size())).arg(name);
    const QString label = move ? tr("Move %n items", "", int(items.size())) : tr("Copy %n items", "", int(items.size()));
    m_jobs->start({{"into", Wire::encode(into)}, {"items", items}, {"label", label}}, label,
                  [this, message](qint64, const BackendError &error) {
                      emit started(error.isNull() ? message : error.message);
                  });
    return true;
}
