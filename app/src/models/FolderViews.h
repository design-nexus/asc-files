// SPDX-License-Identifier: MIT
#pragma once

#include <QHash>
#include <QString>
#include <QStringList>

/// What each folder was last shown as, as legacy Files kept it: its view (List, Icons, Columns)
/// and its sort, by the folder's path, in `$XDG_CONFIG_HOME/asc-files/folder-views.json`. The
/// folders used least lately are forgotten past a bound. A folder never set takes the default
/// sort (Settings › Browsing), and the view the tab is in.
class FolderViews {
public:
    enum View { List, Icons, Columns };
    struct Choice {
        int view = List;
        QString sortKey = QStringLiteral("name");
        bool descending = false;
    };
    static constexpr int MaxFolders = 2000;

    /// The one the whole app shares.
    static FolderViews &shared();

    Choice choice(const QString &folder) const;
    bool has(const QString &folder) const { return m_choices.contains(folder); }
    void remember(const QString &folder, const Choice &choice);
    /// The view and sort a folder without its own takes.
    static Choice defaults();
    int count() const { return int(m_choices.size()); }
    void forgetAll();

    static QString filePath();

private:
    FolderViews();
    void save() const;

    QHash<QString, Choice> m_choices;
    /// Folders by when they were last set, oldest first.
    QStringList m_order;
};
