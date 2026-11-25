// SPDX-License-Identifier: MIT
#include "Operations.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"
#include "../models/FolderModel.h"
#include "../models/JobModel.h"

#include <QClipboard>
#include <QDesktopServices>
#include <QFileInfo>
#include <QHash>
#include <QDir>
#include <QPointer>
#include <QFile>
#include <QGuiApplication>
#include <QMimeData>
#include <QUrl>

namespace {

/// The format GNOME's file managers put a copy or cut in; KDE's marks a cut its own way.
const QString GnomeCopied = QStringLiteral("x-special/gnome-copied-files");
const QString KdeCut = QStringLiteral("application/x-kde-cutselection");

QVariantMap sideMap(const QJsonValue &side) {
    const QJsonObject object = side.toObject();
    return {{QStringLiteral("isFolder"), object.value(QStringLiteral("isFolder")).toBool()},
            {QStringLiteral("size"), object.value(QStringLiteral("size")).toDouble()},
            {QStringLiteral("modified"), object.value(QStringLiteral("modified")).toDouble()}};
}

} // namespace

Operations::Operations(BackendClient *backend, JobModel *jobs, QObject *parent)
    : QObject(parent), m_backend(backend), m_jobs(jobs) {
    connect(QGuiApplication::clipboard(), &QClipboard::dataChanged, this, &Operations::clipboardChanged);
    connect(m_jobs, &JobModel::finished, this, &Operations::refreshHistory);
    connect(m_backend, &BackendClient::ready, this, &Operations::refreshHistory);
}

QList<QByteArray> Operations::selectedPaths(FolderModel *folder) const {
    QList<QByteArray> paths;
    if (folder) {
        for (const QByteArray &name : folder->selectedNames()) {
            paths.append(Wire::join(folder->rawFolder(), name));
        }
    }
    return paths;
}

void Operations::copy(FolderModel *folder, bool cut) { setClipboard(selectedPaths(folder), cut); }

void Operations::copyPaths(const QStringList &paths, bool cut) {
    QList<QByteArray> raw;
    for (const QString &path : paths) {
        raw.append(QFile::encodeName(path));
    }
    setClipboard(raw, cut);
}

void Operations::copyPathText(const QStringList &paths) {
    if (paths.isEmpty()) {
        return;
    }
    QGuiApplication::clipboard()->setText(paths.join(u'\n'));
    emit notice(tr("Copied the location"));
}

void Operations::setClipboard(const QList<QByteArray> &paths, bool cut) {
    if (paths.isEmpty()) {
        return;
    }
    QList<QUrl> urls;
    QByteArray gnome = cut ? "cut" : "copy";
    for (const QByteArray &path : paths) {
        const QUrl url = QUrl::fromLocalFile(QFile::decodeName(path));
        urls.append(url);
        gnome += '\n' + url.toEncoded();
    }
    auto *data = new QMimeData;
    data->setUrls(urls);
    data->setData(GnomeCopied, gnome);
    data->setData(KdeCut, cut ? "1" : "0");
    QGuiApplication::clipboard()->setMimeData(data);
    emit notice(cut ? tr("Cut %n items", "", int(paths.size())) : tr("Copied %n items", "", int(paths.size())));
}

QList<QByteArray> Operations::clipboardPaths(bool *cut) {
    const QMimeData *data = QGuiApplication::clipboard()->mimeData();
    QList<QByteArray> paths;
    if (!data) {
        return paths;
    }
    bool isCut = false;
    if (data->hasFormat(GnomeCopied)) {
        isCut = data->data(GnomeCopied).startsWith("cut");
    } else if (data->hasFormat(KdeCut)) {
        isCut = data->data(KdeCut).trimmed() == "1";
    }
    for (const QUrl &url : data->urls()) {
        if (url.isLocalFile()) {
            paths.append(QFile::encodeName(url.toLocalFile()));
        }
    }
    if (cut) {
        *cut = isCut;
    }
    return paths;
}

bool Operations::canPaste() const { return !clipboardPaths().isEmpty(); }

void Operations::paste(FolderModel *folder, int row) {
    if (!folder || folder->rawFolder().isEmpty()) {
        return;
    }
    bool cut = false;
    const QList<QByteArray> sources = clipboardPaths(&cut);
    if (sources.isEmpty()) {
        return;
    }
    QByteArray into = folder->rawFolder();
    if (row >= 0 && folder->isFolder(row)) {
        into = Wire::join(into, folder->rawName(row));
    }
    transfer(sources, into, cut, true);
}

void Operations::transfer(const QList<QByteArray> &sources, const QByteArray &into, bool cut, bool fromClipboard) {
    QJsonArray wire;
    for (const QByteArray &source : sources) {
        wire.append(Wire::encode(source));
    }
    m_backend->call(QStringLiteral("jobs.plan"), {{"into", Wire::encode(into)}, {"sources", wire}}, this,
                    [this, into, cut, fromClipboard](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit notice(error.message);
                            return;
                        }
                        Pending pending{into, cut, fromClipboard, {}, {}, {}};
                        QVariantList conflicts;
                        const QJsonArray planned = result.toObject().value(QStringLiteral("items")).toArray();
                        // Items landing on the same stored name: as legacy did, one whose own
                        // name needs no fix keeps it (a_b over a:b, which a FAT stick stores as
                        // a_b); the others follow, numbered.
                        QHash<QString, int> winner;
                        for (int i = 0; i < planned.size(); ++i) {
                            const QJsonObject item = planned.at(i).toObject();
                            if (item.contains(QStringLiteral("error"))) {
                                continue;
                            }
                            const QString name = Wire::display(item.value(QStringLiteral("name")));
                            const auto found = winner.constFind(name);
                            if (found == winner.cend() ||
                                (planned.at(*found).toObject().value(QStringLiteral("renamed")).toBool() &&
                                 !item.value(QStringLiteral("renamed")).toBool())) {
                                winner.insert(name, i);
                            }
                        }
                        QJsonArray numbered;
                        for (int i = 0; i < planned.size(); ++i) {
                            const QJsonObject item = planned.at(i).toObject();
                            if (item.contains(QStringLiteral("error"))) {
                                emit notice(item.value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString());
                                continue;
                            }
                            const QJsonValue source = item.value(QStringLiteral("source"));
                            const QJsonValue name = item.value(QStringLiteral("name"));
                            const bool same = item.value(QStringLiteral("same")).toBool();
                            if (same && cut) {
                                continue; // Moved to where it is already.
                            }
                            // Into its own folder, or onto a name another item of the paste
                            // keeps: a copy beside it, numbered, after the others.
                            if (same || winner.value(Wire::display(name)) != i) {
                                numbered.append(QJsonObject{{"source", source},
                                                            {"action", cut ? "move" : "copy"},
                                                            {"conflict", "keepBoth"},
                                                            {"name", name}});
                                continue;
                            }
                            if (item.value(QStringLiteral("existing")).isNull()) {
                                pending.items.append(QJsonObject{{"source", source}, {"action", cut ? "move" : "copy"}});
                                continue;
                            }
                            const bool bothFolders = item.value(QStringLiteral("incoming")).toObject().value(QStringLiteral("isFolder")).toBool() &&
                                                     item.value(QStringLiteral("existing")).toObject().value(QStringLiteral("isFolder")).toBool();
                            conflicts.append(QVariantMap{{QStringLiteral("index"), int(pending.clashes.size())},
                                                         {QStringLiteral("name"), Wire::display(name)},
                                                         {QStringLiteral("folder"), Wire::display(into)},
                                                         {QStringLiteral("incoming"), sideMap(item.value(QStringLiteral("incoming")))},
                                                         {QStringLiteral("existing"), sideMap(item.value(QStringLiteral("existing")))},
                                                         {QStringLiteral("canMerge"), bothFolders}});
                            pending.clashes.append(item);
                        }
                        pending.numbered = numbered;
                        if (conflicts.isEmpty()) {
                            QJsonArray items = pending.items;
                            for (const QJsonValue &item : std::as_const(pending.numbered)) {
                                items.append(item);
                            }
                            startBatch(into, items, cut, fromClipboard);
                            return;
                        }
                        m_pending = pending;
                        m_conflicts = conflicts;
                        emit conflictsChanged();
                    });
}

void Operations::resolve(const QList<int> &choices) {
    Pending pending = std::exchange(m_pending, {});
    m_conflicts.clear();
    emit conflictsChanged();
    if (choices.size() != pending.clashes.size()) {
        return; // Cancelled.
    }
    const bool cut = pending.cut;
    for (int i = 0; i < choices.size(); ++i) {
        const QJsonObject &planned = pending.clashes.at(i);
        const QJsonValue source = planned.value(QStringLiteral("source"));
        // The name the destination stores it under: what Replace replaces.
        const QJsonValue name = planned.value(QStringLiteral("name"));
        switch (choices.at(i)) {
        case Replace:
            pending.items.append(cut ? QJsonObject{{"source", source}, {"action", "move"}, {"conflict", "replace"}, {"name", name}}
                                     : QJsonObject{{"source", source}, {"action", "replace"}, {"name", name}});
            break;
        case KeepBoth:
            pending.items.append(QJsonObject{{"source", source}, {"action", cut ? "move" : "copy"}, {"conflict", "keepBoth"}, {"name", name}});
            break;
        case Merge:
            pending.items.append(QJsonObject{{"source", source}, {"action", "merge"}, {"move", cut}, {"name", name}, {"otherwise", "keepBoth"}});
            break;
        default:
            break;
        }
    }
    // The numbered ones last, once the names they give way to are taken.
    for (const QJsonValue &item : std::as_const(pending.numbered)) {
        pending.items.append(item);
    }
    startBatch(pending.into, pending.items, cut, pending.fromClipboard);
}

void Operations::startBatch(const QByteArray &into, const QJsonArray &items, bool cut, bool fromClipboard) {
    if (items.isEmpty()) {
        return;
    }
    const int count = int(items.size());
    const QString label = cut ? tr("Move %n items", "", count) : tr("Copy %n items", "", count);
    m_jobs->start({{"into", Wire::encode(into)}, {"items", items}, {"label", label}}, label,
                  [this, label](qint64, const BackendError &error) {
                      emit notice(error.isNull() ? label : error.message);
                  });
    if (cut && fromClipboard) {
        // A cut is pasted once.
        QGuiApplication::clipboard()->clear();
    }
}

void Operations::trash(FolderModel *folder) {
    trashRaw(selectedPaths(folder));
    if (folder) {
        folder->clearSelection();
    }
}

void Operations::transferPaths(const QStringList &paths, const QString &folder, bool move) {
    QList<QByteArray> sources;
    for (const QString &path : paths) {
        sources.append(QFile::encodeName(path));
    }
    if (!sources.isEmpty() && !folder.isEmpty()) {
        transfer(sources, QFile::encodeName(folder), move);
    }
}

void Operations::trashPaths(const QStringList &paths) {
    QList<QByteArray> raw;
    for (const QString &path : paths) {
        raw.append(QFile::encodeName(path));
    }
    trashRaw(raw);
}

void Operations::trashRaw(const QList<QByteArray> &paths) {
    QJsonArray items;
    for (const QByteArray &path : paths) {
        items.append(QJsonObject{{"source", Wire::encode(path)}, {"action", "trash"}});
    }
    if (items.isEmpty()) {
        return;
    }
    const QString label = tr("Move %n items to Trash", "", int(items.size()));
    // A job's folder is where its items are; trashed items may come from anywhere.
    m_jobs->start({{"into", Wire::encode(Wire::parent(paths.first()))}, {"items", items}, {"label", label}}, label,
                  [this, label](qint64, const BackendError &error) { emit notice(error.isNull() ? label : error.message); });
}

void Operations::deleteForever(FolderModel *folder) {
    QJsonArray items;
    for (const QByteArray &path : selectedPaths(folder)) {
        items.append(QJsonObject{{"source", Wire::encode(path)}, {"action", "delete"}});
    }
    if (items.isEmpty()) {
        return;
    }
    const QString label = tr("Delete %n items", "", int(items.size()));
    m_jobs->start({{"into", Wire::encode(folder->rawFolder())}, {"items", items}, {"label", label}}, label,
                  [this, label](qint64, const BackendError &error) { emit notice(error.isNull() ? label : error.message); });
    folder->clearSelection();
}

void Operations::create(FolderModel *folder, bool isFolder) {
    if (!folder || folder->rawFolder().isEmpty()) {
        return;
    }
    m_backend->call(QStringLiteral("ops.create"),
                    {{"into", Wire::encode(folder->rawFolder())},
                     {"name", isFolder ? tr("New Folder") : tr("New File")},
                     {"kind", isFolder ? "folder" : "file"},
                     {"unique", true}},
                    this, [this, folder = QPointer<FolderModel>(folder)](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit notice(error.message);
                            return;
                        }
                        refreshHistory();
                        if (folder) {
                            emit created(folder, Wire::display(Wire::baseName(Wire::decode(result.toObject().value(QStringLiteral("path"))))));
                        }
                    });
}

void Operations::restore(const QString &file) {
    const QString label = tr("Restore");
    // A restore ignores where it is put; the Trash's record says.
    m_jobs->start({{"into", Wire::encode(QFile::encodeName(QDir::homePath()))},
                   {"items", QJsonArray{QJsonObject{{"source", Wire::encode(QFile::encodeName(file))}, {"action", "restore"}}}},
                   {"label", label}},
                  label, [this](qint64, const BackendError &error) {
                      if (!error.isNull()) {
                          emit notice(error.message);
                      }
                  });
}

void Operations::deleteFromTrash(const QString &info) {
    m_backend->call(QStringLiteral("trash.delete"), {{"info", Wire::encode(QFile::encodeName(info))}}, this,
                    [this](const QJsonValue &, const BackendError &error) {
                        emit notice(error.isNull() ? tr("Deleted for good") : error.message);
                        emit trashChanged();
                    });
}

void Operations::emptyTrash() {
    m_backend->call(QStringLiteral("trash.empty"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            emit notice(error.message);
        } else {
            const QJsonObject emptied = result.toObject();
            const QJsonArray failed = emptied.value(QStringLiteral("failed")).toArray();
            emit notice(failed.isEmpty()
                            ? tr("The Trash is empty")
                            : tr("%n items could not be deleted", "", int(failed.size())) + QStringLiteral(": ") +
                                  failed.first().toObject().value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString());
        }
        emit trashChanged();
    });
}

void Operations::reviewLeftovers(FolderModel *folder) {
    if (!folder || folder->rawLeftovers().isEmpty()) {
        return;
    }
    QJsonArray names;
    for (const QByteArray &name : folder->rawLeftovers()) {
        names.append(Wire::encode(name));
    }
    m_backend->call(QStringLiteral("leftovers.review"), {{"folder", Wire::encode(folder->rawFolder())}, {"names", names}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit notice(error.message);
                            return;
                        }
                        QVariantList leftovers;
                        for (const QJsonValue &value : result.toObject().value(QStringLiteral("leftovers")).toArray()) {
                            const QJsonObject leftover = value.toObject();
                            const QJsonObject inside = leftover.value(QStringLiteral("inside")).toObject();
                            QStringList inner;
                            for (const QJsonValue &name : inside.value(QStringLiteral("names")).toArray()) {
                                inner.append(Wire::display(name));
                            }
                            leftovers.append(QVariantMap{
                                {QStringLiteral("name"), Wire::display(leftover.value(QStringLiteral("name")))},
                                {QStringLiteral("modified"), leftover.value(QStringLiteral("modified")).toDouble()},
                                {QStringLiteral("kind"), inside.value(QStringLiteral("kind")).toString()},
                                {QStringLiteral("names"), inner},
                                {QStringLiteral("more"), inside.value(QStringLiteral("more")).toInt()},
                                {QStringLiteral("reason"), inside.value(QStringLiteral("reason")).toString()}});
                        }
                        emit leftoversReviewed(leftovers);
                    });
}

void Operations::recover(FolderModel *folder, const QString &name) {
    if (!folder) {
        return;
    }
    QByteArray raw;
    for (const QByteArray &leftover : folder->rawLeftovers()) {
        if (Wire::display(leftover) == name) {
            raw = leftover;
        }
    }
    if (raw.isEmpty()) {
        return;
    }
    m_backend->call(QStringLiteral("leftovers.recover"), {{"path", Wire::encode(Wire::join(folder->rawFolder(), raw))}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit notice(error.message);
                            return;
                        }
                        const QJsonObject recovered = result.toObject();
                        const int restored = int(recovered.value(QStringLiteral("restored")).toArray().size() +
                                                 recovered.value(QStringLiteral("renamed")).toArray().size());
                        const QJsonArray failed = recovered.value(QStringLiteral("failed")).toArray();
                        QString message = tr("Put back %n items", "", restored);
                        if (!recovered.value(QStringLiteral("renamed")).toArray().isEmpty()) {
                            message += QStringLiteral(" · ") + tr("some under new names, their own being taken");
                        }
                        if (!failed.isEmpty()) {
                            message += QStringLiteral(" · ") + failed.first().toObject().value(QStringLiteral("error")).toString();
                        }
                        emit notice(message);
                    });
}

void Operations::open(const QString &path) {
    if (!QDesktopServices::openUrl(QUrl::fromLocalFile(path))) {
        emit notice(tr("No program is set to open “%1”").arg(QFileInfo(path).fileName()));
    }
}

void Operations::undo(bool overwrite) {
    if (!overwrite) {
        // Asked first: would undo meet names newer items hold?
        m_backend->call(QStringLiteral("undo.preview"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
            const QJsonArray blocked = result.toObject().value(QStringLiteral("blocked")).toArray();
            if (!error.isNull() || blocked.isEmpty()) {
                undoNow(false);
                return;
            }
            QStringList names;
            for (const QJsonValue &path : blocked) {
                names.append(Wire::display(Wire::baseName(Wire::decode(path))));
            }
            emit undoBlocked(names);
        });
        return;
    }
    undoNow(true);
}

void Operations::undoNow(bool overwrite) {
    m_backend->call(QStringLiteral("undo.undo"), {{"overwrite", overwrite}}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            emit notice(error.kind == QLatin1String("notFound") ? tr("There is nothing to undo") : error.message);
            return;
        }
        const QJsonObject undone = result.toObject();
        const QJsonArray left = undone.value(QStringLiteral("left")).toArray();
        QString message = tr("Undid “%1”").arg(undone.value(QStringLiteral("label")).toString());
        if (!left.isEmpty()) {
            // What could not be undone, and why: the first reason, and how many more.
            message += QStringLiteral(" · ") + tr("%n items left as they were", "", int(left.size())) + QStringLiteral(": ") +
                       left.first().toObject().value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString();
        }
        emit notice(message);
        refreshHistory();
        emit trashChanged();
    });
}

void Operations::redo() {
    m_backend->call(QStringLiteral("undo.redo"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
        if (!error.isNull()) {
            emit notice(error.kind == QLatin1String("notFound") ? tr("There is nothing to redo") : error.message);
            return;
        }
        emit notice(tr("Redid “%1”").arg(result.toObject().value(QStringLiteral("label")).toString()));
        refreshHistory();
    });
}

void Operations::refreshHistory() {
    m_backend->call(QStringLiteral("undo.history"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
        const QJsonObject history = error.isNull() ? result.toObject() : QJsonObject();
        m_undoLabel = history.value(QStringLiteral("undo")).toObject().value(QStringLiteral("label")).toString();
        m_redoLabel = history.value(QStringLiteral("redo")).toObject().value(QStringLiteral("label")).toString();
        emit historyChanged();
    });
}
