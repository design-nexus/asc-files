// SPDX-License-Identifier: MIT
#pragma once

#include "ItemListModel.h"

#include <QFileSystemWatcher>

/// The files used lately, newest first, from the desktop's shared record
/// (`$XDG_DATA_HOME/recently-used.xbel`), which every program that opens files writes to. Only
/// local files are listed. The list follows the record as programs add to it.
class RecentModel : public ItemListModel {
    Q_OBJECT
public:
    static constexpr int MaxItems = 200;

    explicit RecentModel(QObject *parent = nullptr);
    Q_INVOKABLE void reload();

    /// The record's path.
    static QString recordPath();

private:
    QFileSystemWatcher m_watch;
};
