// SPDX-License-Identifier: MIT
#pragma once

#include <QHash>
#include <QObject>
#include <QStringList>
#include <QVariantList>

/// The desktop's programs, read as the freedesktop specifications have them: each `.desktop`
/// entry under `applications/` in the data folders (the earliest folder wins an ID), and which
/// opens what from the `mimeapps.list` files (defaults, added and removed associations, the
/// desktop's own list first) and the entries' own `MimeType`s, a type's parents included.
/// Open With asks it what can open the selected files, and starts the program chosen.
class DesktopApps : public QObject {
    Q_OBJECT
public:
    struct App {
        QString id;          ///< `org.gnome.Loupe.desktop`
        QString file;        ///< Where its entry is.
        QString name;        ///< In the user's language when the entry has it.
        QString icon;        ///< A theme icon's name, or a picture's path.
        QString exec;
        QString path;        ///< The folder it starts in.
        QStringList mimeTypes;
        bool terminal = false;
        bool noDisplay = false;
    };

    explicit DesktopApps(QObject *parent = nullptr);

    /// What can open every one of `paths`, as `[{id, name, icon, isDefault}]`: the default first
    /// (when all the files share it), then the others that say they open such files. At most
    /// `max` (all when negative).
    Q_INVOKABLE QVariantList appsFor(const QStringList &paths, int max = -1);
    /// The programs that don't say they open these files, by name, for "Other Application…".
    Q_INVOKABLE QVariantList otherApps(const QStringList &paths);
    /// Starts `id` on `paths`; empty, or why it couldn't.
    Q_INVOKABLE QString launch(const QString &id, const QStringList &paths);
    /// Makes `id` the default for the types of `paths`, in the user's `mimeapps.list`.
    Q_INVOKABLE bool setDefault(const QString &id, const QStringList &paths);
    /// The types of `paths`, once each, as their description says them ("PNG image").
    Q_INVOKABLE QString typesText(const QStringList &paths) const;

    /// A parsed entry; `ok` false when it isn't a program to show or start.
    static App parseEntry(const QString &file, const QString &id, const QByteArray &contents, const QString &language, bool *ok);
    /// The arguments to start `app` with, once for each list returned (a program taking one file
    /// at a time starts once per file); empty when `exec` can't be read.
    static QList<QStringList> commandLines(const App &app, const QStringList &paths);
    /// `Exec` split into arguments, as the specification quotes them.
    static QStringList splitExec(const QString &exec, bool *ok);

    /// The user's own list, where a default chosen here is kept.
    static QString userListPath();
    /// `contents` of a `mimeapps.list` with `id` the default for `types`, and first among their
    /// added associations; other lines and groups untouched.
    static QString withDefault(const QString &contents, const QString &id, const QStringList &types);

private:
    struct Associations {
        QHash<QString, QStringList> defaults;
        QHash<QString, QStringList> added;
        QHash<QString, QStringList> removed;
    };
    void refresh();
    QStringList mimeTypesOf(const QStringList &paths) const;
    /// For one type: the default (empty if none), then the rest, best first.
    QPair<QString, QStringList> handlers(const QString &type) const;
    QVariantMap entry(const App &app, bool isDefault) const;

    QHash<QString, App> m_apps;
    Associations m_associations;
    /// When each applications folder last changed, to read them again when one does.
    QHash<QString, qint64> m_stamps;
};
