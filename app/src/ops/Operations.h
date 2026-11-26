// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QObject>
#include <QVariantList>

class BackendClient;
class FolderModel;
class JobModel;

/// The window's file operations: the clipboard (Copy, Cut, Paste), moving to the Trash and
/// deleting, new folders and files, and undo and redo through the backend's history.
///
/// Paste asks the backend first what each item would meet (`jobs.plan`, by the names the
/// destination's drive would store them under). Items that meet nothing go ahead; the others
/// wait for the conflict prompt, which answers each with replace, keep both, skip or (two
/// folders) merge, and then the batch starts as one job.
class Operations : public QObject {
    Q_OBJECT
    /// The clashes a paste is waiting on the prompt for: `[{index, name, folder, incoming,
    /// existing, canMerge}]`, each side `{isFolder, size, modified}`.
    Q_PROPERTY(QVariantList conflicts READ conflicts NOTIFY conflictsChanged)
    Q_PROPERTY(bool canPaste READ canPaste NOTIFY clipboardChanged)
    Q_PROPERTY(QString undoLabel READ undoLabel NOTIFY historyChanged)
    Q_PROPERTY(QString redoLabel READ redoLabel NOTIFY historyChanged)
public:
    enum Choice { Replace, KeepBoth, Skip, Merge };
    Q_ENUM(Choice)

    Operations(BackendClient *backend, JobModel *jobs, QObject *parent = nullptr);

    /// Puts the selection of `folder` on the clipboard, to be copied, or moved when `cut`.
    Q_INVOKABLE void copy(FolderModel *folder, bool cut);
    /// Copies (or cuts) items named by path: from Recent, Favorites, search results, a crumb.
    Q_INVOKABLE void copyPaths(const QStringList &paths, bool cut);
    /// Puts the paths themselves on the clipboard, as text.
    Q_INVOKABLE void copyPathText(const QStringList &paths);
    /// Takes back what Files put on the clipboard (another program's stays).
    Q_INVOKABLE void clearClipboard();
    /// Copies (or moves) items named by path into `folder`, as a paste does: Dual Pane's F5 and F6.
    Q_INVOKABLE void transferPaths(const QStringList &paths, const QString &folder, bool move);
    /// Moves items named by path to the Trash.
    Q_INVOKABLE void trashPaths(const QStringList &paths);
    /// Pastes the clipboard into the folder `folder` shows, or into its folder at `row` when
    /// `row` is a folder.
    Q_INVOKABLE void paste(FolderModel *folder, int row = -1);
    /// Moves (else copies) `sources` into `into` as a paste does: planned by the names the
    /// destination stores, a clash asked about. A drop comes this way too; only a paste of a
    /// cut clears the clipboard.
    void transfer(const QList<QByteArray> &sources, const QByteArray &into, bool move, bool fromClipboard = false);
    /// Answers the waiting paste's clashes, one choice per clash in `conflicts` order, and
    /// starts it. An empty list cancels it.
    Q_INVOKABLE void resolve(const QList<int> &choices);
    /// Moves the selection of `folder` to the Trash.
    Q_INVOKABLE void trash(FolderModel *folder);
    /// Deletes the selection of `folder` for good (the window asks first).
    Q_INVOKABLE void deleteForever(FolderModel *folder);
    /// Makes a new folder (or file) in `folder`'s folder under the first free default name and
    /// starts renaming it.
    Q_INVOKABLE void create(FolderModel *folder, bool isFolder);
    /// Puts a Trash entry back where it was deleted from (`file` is the item in the Trash).
    Q_INVOKABLE void restore(const QString &file);
    /// Deletes a Trash entry for good, by its `.trashinfo` (the window asks first).
    Q_INVOKABLE void deleteFromTrash(const QString &info);
    /// Deletes everything in every Trash for good (the window asks first).
    Q_INVOKABLE void emptyTrash();
    /// Says what each leftover in `folder` holds, answered by `leftoversReviewed`.
    Q_INVOKABLE void reviewLeftovers(FolderModel *folder);
    /// Puts the originals in the leftover `name` of `folder` back under their names.
    Q_INVOKABLE void recover(FolderModel *folder, const QString &name);
    /// Undoes the newest entry. Where it would put an item back under a name something else
    /// holds now, it asks first (`undoBlocked`); `overwrite`, once confirmed, moves those to the
    /// Trash and goes ahead.
    Q_INVOKABLE void undo(bool overwrite = false);
    /// Opens a file in the program the desktop opens its kind with.
    /// Opens `path` with its program; an archive is extracted beside itself instead, as legacy did.
    Q_INVOKABLE void open(const QString &path);
    /// `zip`, `7z`, `tar`, `tar.gz` or `rar` (extracted, never made) for an archive Files
    /// extracts, by its name; empty otherwise.
    Q_INVOKABLE static QString archiveFormat(const QString &path);
    /// Extracts `archive` into `folder` (its own folder when empty). A locked one asks for its
    /// password through `passwordWanted`.
    Q_INVOKABLE void extract(const QString &archive, const QString &folder = QString(), const QString &password = QString());
    /// Makes `name` plus `format`'s extension of `paths`, in the first one's folder, numbered if
    /// the name is taken.
    Q_INVOKABLE void compress(const QStringList &paths, const QString &name, const QString &format, const QString &password = QString());
    Q_INVOKABLE void redo();
    Q_INVOKABLE void refreshHistory();

    QVariantList conflicts() const { return m_conflicts; }
    bool canPaste() const;
    QString undoLabel() const { return m_undoLabel; }
    QString redoLabel() const { return m_redoLabel; }

    /// The paths a clipboard names, from `text/uri-list`.
    static QList<QByteArray> clipboardPaths(bool *cut = nullptr);

signals:
    void conflictsChanged();
    void clipboardChanged();
    void historyChanged();
    /// Something to tell the user: what started, what was undone, why something was refused.
    void notice(const QString &message);
    /// What each leftover holds: `[{name, modified, kind, names, more, reason}]`.
    void leftoversReviewed(const QVariantList &leftovers);
    /// Undo would put items back where `names` (newer items) are now: the window asks.
    void undoBlocked(const QStringList &names);
    /// The Trash changed through Files.
    void trashChanged();
    /// A new item was made in `folder`; the window starts renaming it.
    void created(FolderModel *folder, const QString &name);
    /// `archive` is locked: the window asks for the password (again, when `incorrect`) and calls
    /// `extract` with it.
    void passwordWanted(const QString &archive, const QString &folder, bool incorrect);
    /// An archive was made, or one extracted into `path`: the window selects it.
    void archived(const QString &path);

private:
    struct Pending {
        QByteArray into;
        bool cut = false;
        bool fromClipboard = false;
        QJsonArray items;
        /// Per clash: the plan's item for it.
        QList<QJsonObject> clashes;
        /// Items numbered beside a name another item keeps; started after the rest.
        QJsonArray numbered;
    };
    void startBatch(const QByteArray &into, const QJsonArray &items, bool cut, bool fromClipboard);
    void undoNow(bool overwrite);
    QList<QByteArray> selectedPaths(FolderModel *folder) const;
    void setClipboard(const QList<QByteArray> &paths, bool cut);
    void trashRaw(const QList<QByteArray> &paths);
    void archiveItemDone(qint64 job, const QJsonObject &outcome);

    BackendClient *m_backend;
    JobModel *m_jobs;
    Pending m_pending;
    /// Archive jobs running: the archive and where it goes, for a password asked for.
    struct ArchiveJob {
        QString archive;
        QString folder;
    };
    QHash<qint64, ArchiveJob> m_archiveJobs;
    QVariantList m_conflicts;
    QString m_undoLabel;
    QString m_redoLabel;
};
