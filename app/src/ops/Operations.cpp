// SPDX-License-Identifier: MIT
#include "Operations.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"
#include "../models/FolderModel.h"
#include "../models/JobModel.h"

#include <QClipboard>
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

void Operations::copy(FolderModel *folder, bool cut) {
    const QList<QByteArray> paths = selectedPaths(folder);
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
    QJsonArray wire;
    for (const QByteArray &source : sources) {
        wire.append(Wire::encode(source));
    }
    m_backend->call(QStringLiteral("jobs.plan"), {{"into", Wire::encode(into)}, {"sources", wire}}, this,
                    [this, into, cut](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            emit notice(error.message);
                            return;
                        }
                        Pending pending{into, cut, {}, {}};
                        QVariantList conflicts;
                        for (const QJsonValue &value : result.toObject().value(QStringLiteral("items")).toArray()) {
                            const QJsonObject planned = value.toObject();
                            if (planned.contains(QStringLiteral("error"))) {
                                emit notice(planned.value(QStringLiteral("error")).toObject().value(QStringLiteral("message")).toString());
                                continue;
                            }
                            const QJsonValue source = planned.value(QStringLiteral("source"));
                            const QJsonValue name = planned.value(QStringLiteral("name"));
                            const bool same = planned.value(QStringLiteral("same")).toBool();
                            if (same && cut) {
                                continue; // Moved to where it is already.
                            }
                            // Into its own folder, or onto a name an earlier item takes: a copy
                            // beside it, as no one would want the one replaced by the other.
                            if (same || !planned.value(QStringLiteral("aliasOf")).isNull()) {
                                pending.items.append(QJsonObject{{"source", source},
                                                                 {"action", cut ? "move" : "copy"},
                                                                 {"conflict", "keepBoth"},
                                                                 {"name", name}});
                                continue;
                            }
                            if (planned.value(QStringLiteral("existing")).isNull()) {
                                pending.items.append(QJsonObject{{"source", source}, {"action", cut ? "move" : "copy"}});
                                continue;
                            }
                            const bool bothFolders = planned.value(QStringLiteral("incoming")).toObject().value(QStringLiteral("isFolder")).toBool() &&
                                                     planned.value(QStringLiteral("existing")).toObject().value(QStringLiteral("isFolder")).toBool();
                            conflicts.append(QVariantMap{{QStringLiteral("index"), int(pending.clashes.size())},
                                                         {QStringLiteral("name"), Wire::display(name)},
                                                         {QStringLiteral("folder"), Wire::display(into)},
                                                         {QStringLiteral("incoming"), sideMap(planned.value(QStringLiteral("incoming")))},
                                                         {QStringLiteral("existing"), sideMap(planned.value(QStringLiteral("existing")))},
                                                         {QStringLiteral("canMerge"), bothFolders}});
                            pending.clashes.append(planned);
                        }
                        if (conflicts.isEmpty()) {
                            startBatch(into, pending.items, cut);
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
    startBatch(pending.into, pending.items, cut);
}

void Operations::startBatch(const QByteArray &into, const QJsonArray &items, bool cut) {
    if (items.isEmpty()) {
        return;
    }
    const int count = int(items.size());
    const QString label = cut ? tr("Move %n items", "", count) : tr("Copy %n items", "", count);
    m_jobs->start({{"into", Wire::encode(into)}, {"items", items}, {"label", label}}, label,
                  [this, label](qint64, const BackendError &error) {
                      emit notice(error.isNull() ? label : error.message);
                  });
    if (cut) {
        // A cut is pasted once.
        QGuiApplication::clipboard()->clear();
    }
}

void Operations::trash(FolderModel *folder) {
    QJsonArray items;
    for (const QByteArray &path : selectedPaths(folder)) {
        items.append(QJsonObject{{"source", Wire::encode(path)}, {"action", "trash"}});
    }
    if (items.isEmpty()) {
        return;
    }
    const QString label = tr("Move %n items to Trash", "", int(items.size()));
    m_jobs->start({{"into", Wire::encode(folder->rawFolder())}, {"items", items}, {"label", label}}, label,
                  [this, label](qint64, const BackendError &error) { emit notice(error.isNull() ? label : error.message); });
    folder->clearSelection();
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

void Operations::undo() {
    m_backend->call(QStringLiteral("undo.undo"), {}, this, [this](const QJsonValue &result, const BackendError &error) {
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
