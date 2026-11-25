// SPDX-License-Identifier: MIT
#pragma once

#include <QList>
#include <QObject>
#include <QUrl>

class FolderModel;
class Operations;

/// Drag and drop: what a drag from a folder carries, and what a drop does. A drop on the drive
/// its items are on moves them; one onto another drive does what Settings › Transfers says:
/// copy, move, or ask (legacy's default), when the window that took the drop asks and answers.
/// It goes as a paste does (Operations::transfer): a name that is taken is asked about.
class Transfers : public QObject {
    Q_OBJECT
public:
    /// What a drop did.
    enum Dropped { Refused, Started, Asking };
    Q_ENUM(Dropped)

    explicit Transfers(Operations *ops, QObject *parent = nullptr);

    /// The `text/uri-list` a drag from `row` carries: the selection, or `row` alone (selected
    /// now) when it is not part of it.
    Q_INVOKABLE QString dragUris(FolderModel *folder, int row) const;
    /// Whether a drop of `urls` into `target` would move them (else copy).
    Q_INVOKABLE bool movesInto(const QList<QUrl> &urls, const QString &target) const;
    /// Drops `urls` into the folder `target`. Items already in it, and a folder dropped into
    /// itself, are left out: Refused when nothing was left to do. Asking when it crosses drives
    /// and Settings says to ask: `answer()` then says what to do.
    Q_INVOKABLE int drop(const QList<QUrl> &urls, const QString &target);
    /// Drops into the folder at `row` of `folder`'s listing (by its raw name).
    Q_INVOKABLE int dropInto(const QList<QUrl> &urls, FolderModel *folder, int row);
    /// Drops into the folder `folder` lists.
    Q_INVOKABLE int dropHere(const QList<QUrl> &urls, FolderModel *folder);
    /// The answer for a drop that is Asking: copied, moved, or (neither) dropped no more.
    Q_INVOKABLE void answer(bool copy, bool move);

private:
    int dropRaw(const QList<QUrl> &urls, const QByteArray &into);
    Operations *m_ops;
    /// The drop waiting on its answer.
    QList<QByteArray> m_askedSources;
    QByteArray m_askedInto;
};
