// SPDX-License-Identifier: MIT
#pragma once

#include <QList>
#include <QObject>
#include <QUrl>

class FolderModel;
class Operations;

/// Drag and drop: what a drag from a folder carries, and what a drop does. A drop on the drive
/// its items are on moves them; a drop on another drive copies them, as file managers do. It
/// goes as a paste does (Operations::transfer): a name that is taken is asked about.
class Transfers : public QObject {
    Q_OBJECT
public:
    explicit Transfers(Operations *ops, QObject *parent = nullptr);

    /// The `text/uri-list` a drag from `row` carries: the selection, or `row` alone (selected
    /// now) when it is not part of it.
    Q_INVOKABLE QString dragUris(FolderModel *folder, int row) const;
    /// Whether a drop of `urls` into `target` would move them (else copy).
    Q_INVOKABLE bool movesInto(const QList<QUrl> &urls, const QString &target) const;
    /// Drops `urls` into the folder `target`: moved there on its drive, copied from another.
    /// Items already in it, and a folder dropped into itself, are left out. False when nothing
    /// was left to do.
    Q_INVOKABLE bool drop(const QList<QUrl> &urls, const QString &target);
    /// Drops into the folder at `row` of `folder`'s listing (by its raw name).
    Q_INVOKABLE bool dropInto(const QList<QUrl> &urls, FolderModel *folder, int row);
    /// Drops into the folder `folder` lists.
    Q_INVOKABLE bool dropHere(const QList<QUrl> &urls, FolderModel *folder);

private:
    bool dropRaw(const QList<QUrl> &urls, const QByteArray &into);
    Operations *m_ops;
};
