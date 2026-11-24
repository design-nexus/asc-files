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
    /// Pastes the clipboard into the folder `folder` shows, or into its folder at `row` when
    /// `row` is a folder.
    Q_INVOKABLE void paste(FolderModel *folder, int row = -1);
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

private:
    struct Pending {
        QByteArray into;
        bool cut = false;
        QJsonArray items;
        /// Per clash: the plan's item for it.
        QList<QJsonObject> clashes;
        /// Items numbered beside a name another item keeps; started after the rest.
        QJsonArray numbered;
    };
    void startBatch(const QByteArray &into, const QJsonArray &items, bool cut);
    void undoNow(bool overwrite);
    QList<QByteArray> selectedPaths(FolderModel *folder) const;

    BackendClient *m_backend;
    JobModel *m_jobs;
    Pending m_pending;
    QVariantList m_conflicts;
    QString m_undoLabel;
    QString m_redoLabel;
};
