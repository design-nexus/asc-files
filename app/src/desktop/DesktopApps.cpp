// SPDX-License-Identifier: MIT
#include "DesktopApps.h"

#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QLocale>
#include <QMimeDatabase>
#include <QProcess>
#include <QSaveFile>
#include <QSet>
#include <QStandardPaths>
#include <QUrl>

#include <algorithm>

namespace {

/// A value as the specification escapes it: `\s`, `\n`, `\t`, `\r`, `\\`.
QString unescape(const QString &value) {
    QString out;
    out.reserve(value.size());
    for (qsizetype i = 0; i < value.size(); ++i) {
        const QChar c = value.at(i);
        if (c != u'\\' || i + 1 == value.size()) {
            out.append(c);
            continue;
        }
        const QChar next = value.at(++i);
        switch (next.unicode()) {
        case 's': out.append(u' '); break;
        case 'n': out.append(u'\n'); break;
        case 't': out.append(u'\t'); break;
        case 'r': out.append(u'\r'); break;
        case '\\': out.append(u'\\'); break;
        // Others (`\;` in lists, and Exec's own quoting) are left for whoever reads further.
        default: out.append(u'\\').append(next); break;
        }
    }
    return out;
}

QStringList list(const QString &value) {
    QStringList items;
    for (const QString &item : value.split(u';')) {
        if (!item.trimmed().isEmpty()) {
            items.append(item.trimmed());
        }
    }
    return items;
}

/// The desktops named in `XDG_CURRENT_DESKTOP`, lowercased, for `<desktop>-mimeapps.list`.
QStringList currentDesktops() {
    QStringList desktops;
    for (const QString &desktop : qEnvironmentVariable("XDG_CURRENT_DESKTOP").split(u':', Qt::SkipEmptyParts)) {
        desktops.append(desktop);
    }
    return desktops;
}

bool isExecutable(const QString &program) {
    if (program.contains(u'/')) {
        const QFileInfo info(program);
        return info.isFile() && info.isExecutable();
    }
    return !QStandardPaths::findExecutable(program).isEmpty();
}

/// A terminal to start a `Terminal=true` program in, before its arguments.
QStringList terminal() {
    if (isExecutable(QStringLiteral("xdg-terminal-exec"))) {
        return {QStringLiteral("xdg-terminal-exec")};
    }
    const QString chosen = qEnvironmentVariable("TERMINAL");
    if (!chosen.isEmpty() && isExecutable(chosen)) {
        return {chosen, QStringLiteral("-e")};
    }
    const QList<QStringList> known{{"foot"}, {"kitty"}, {"alacritty", "-e"}, {"wezterm", "start", "--"},
                                   {"konsole", "-e"}, {"gnome-terminal", "--"}, {"xterm", "-e"}};
    for (const QStringList &candidate : known) {
        if (isExecutable(candidate.first())) {
            return candidate;
        }
    }
    return {};
}

} // namespace

DesktopApps::DesktopApps(QObject *parent) : QObject(parent) {}

DesktopApps::App DesktopApps::parseEntry(const QString &file, const QString &id, const QByteArray &contents,
                                         const QString &language, bool *ok) {
    App app;
    app.id = id;
    app.file = file;
    *ok = false;
    const QString shortLanguage = language.section(u'_', 0, 0);
    QString exactName, shortName, plainName, type, tryExec, onlyShowIn, notShowIn;
    bool hidden = false;
    bool inEntry = false;
    for (const QByteArray &raw : contents.split('\n')) {
        const QString line = QString::fromUtf8(raw).trimmed();
        if (line.isEmpty() || line.startsWith(u'#')) {
            continue;
        }
        if (line.startsWith(u'[')) {
            inEntry = line == QLatin1String("[Desktop Entry]");
            continue;
        }
        const qsizetype equals = line.indexOf(u'=');
        if (!inEntry || equals <= 0) {
            continue;
        }
        const QString key = line.left(equals).trimmed();
        const QString value = line.mid(equals + 1).trimmed();
        if (key == QLatin1String("Name")) plainName = unescape(value);
        else if (key == QStringLiteral("Name[%1]").arg(language)) exactName = unescape(value);
        else if (key == QStringLiteral("Name[%1]").arg(shortLanguage)) shortName = unescape(value);
        else if (key == QLatin1String("Type")) type = value;
        else if (key == QLatin1String("Exec")) app.exec = unescape(value);
        else if (key == QLatin1String("TryExec")) tryExec = unescape(value);
        else if (key == QLatin1String("Icon")) app.icon = unescape(value);
        else if (key == QLatin1String("Path")) app.path = unescape(value);
        else if (key == QLatin1String("MimeType")) app.mimeTypes = list(value);
        else if (key == QLatin1String("Terminal")) app.terminal = value == QLatin1String("true");
        else if (key == QLatin1String("NoDisplay")) app.noDisplay = value == QLatin1String("true");
        else if (key == QLatin1String("Hidden")) hidden = value == QLatin1String("true");
        else if (key == QLatin1String("OnlyShowIn")) onlyShowIn = value;
        else if (key == QLatin1String("NotShowIn")) notShowIn = value;
    }
    app.name = !exactName.isEmpty() ? exactName : !shortName.isEmpty() ? shortName : plainName;
    // Shown only on some desktops: still a handler, not offered among all programs.
    const QStringList desktops = currentDesktops();
    const auto onAny = [&](const QString &names) {
        return std::any_of(desktops.cbegin(), desktops.cend(), [&](const QString &d) { return list(names).contains(d); });
    };
    if ((!onlyShowIn.isEmpty() && !onAny(onlyShowIn)) || (!notShowIn.isEmpty() && onAny(notShowIn))) {
        app.noDisplay = true;
    }
    *ok = type == QLatin1String("Application") && !hidden && !app.exec.isEmpty() && !app.name.isEmpty() &&
          (tryExec.isEmpty() || isExecutable(tryExec));
    return app;
}

QStringList DesktopApps::splitExec(const QString &exec, bool *ok) {
    QStringList arguments;
    QString current;
    bool quoted = false;
    bool started = false;
    for (qsizetype i = 0; i < exec.size(); ++i) {
        const QChar c = exec.at(i);
        if (quoted) {
            if (c == u'\\' && i + 1 < exec.size() && QStringLiteral("\"`$\\").contains(exec.at(i + 1))) {
                current.append(exec.at(++i));
            } else if (c == u'"') {
                quoted = false;
            } else {
                current.append(c);
            }
        } else if (c == u'"') {
            quoted = true;
            started = true;
        } else if (c.isSpace()) {
            if (started) {
                arguments.append(current);
                current.clear();
                started = false;
            }
        } else {
            current.append(c);
            started = true;
        }
    }
    if (started) {
        arguments.append(current);
    }
    *ok = !quoted && !arguments.isEmpty();
    return arguments;
}

QList<QStringList> DesktopApps::commandLines(const App &app, const QStringList &paths) {
    bool ok = false;
    const QStringList tokens = splitExec(app.exec, &ok);
    if (!ok) {
        return {};
    }
    const auto url = [](const QString &path) { return QString::fromUtf8(QUrl::fromLocalFile(path).toEncoded()); };
    // One file at a time (%f, %u) starts the program once for each.
    const bool takesMany = tokens.contains(QStringLiteral("%F")) || tokens.contains(QStringLiteral("%U"));
    const bool takesOne = !takesMany && std::any_of(tokens.cbegin(), tokens.cend(), [](const QString &t) {
        return t.contains(QStringLiteral("%f")) || t.contains(QStringLiteral("%u"));
    });
    const QList<QStringList> runs = takesOne && paths.size() > 1
                                        ? [&] { QList<QStringList> each; for (const QString &p : paths) each.append({p}); return each; }()
                                        : QList<QStringList>{paths};
    QList<QStringList> lines;
    for (const QStringList &files : runs) {
        QStringList line;
        for (const QString &token : tokens) {
            if (token == QLatin1String("%F")) {
                line.append(files);
            } else if (token == QLatin1String("%U")) {
                for (const QString &file : files) line.append(url(file));
            } else if (token == QLatin1String("%f") || token == QLatin1String("%u")) {
                if (!files.isEmpty()) line.append(token == QLatin1String("%f") ? files.first() : url(files.first()));
            } else if (token == QLatin1String("%i")) {
                if (!app.icon.isEmpty()) line << QStringLiteral("--icon") << app.icon;
            } else {
                // Codes inside an argument; deprecated ones go.
                QString out;
                for (qsizetype i = 0; i < token.size(); ++i) {
                    if (token.at(i) != u'%' || i + 1 == token.size()) {
                        out.append(token.at(i));
                        continue;
                    }
                    switch (token.at(++i).unicode()) {
                    case '%': out.append(u'%'); break;
                    case 'f': if (!files.isEmpty()) out.append(files.first()); break;
                    case 'u': if (!files.isEmpty()) out.append(url(files.first())); break;
                    case 'c': out.append(app.name); break;
                    case 'k': out.append(app.file); break;
                    default: break;
                    }
                }
                if (!out.isEmpty() || token.isEmpty()) {
                    line.append(out);
                }
            }
        }
        if (app.terminal) {
            const QStringList in = terminal();
            if (in.isEmpty()) {
                return {};
            }
            line = in + line;
        }
        if (!line.isEmpty()) {
            lines.append(line);
        }
    }
    return lines;
}

QString DesktopApps::userListPath() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/mimeapps.list");
}

void DesktopApps::refresh() {
    const QStringList folders = QStandardPaths::standardLocations(QStandardPaths::ApplicationsLocation);
    QHash<QString, qint64> stamps;
    for (const QString &folder : folders) {
        stamps.insert(folder, QFileInfo(folder).lastModified().toMSecsSinceEpoch());
    }
    if (stamps != m_stamps) {
        m_stamps = stamps;
        m_apps.clear();
        QSet<QString> seen;
        const QString language = QLocale().name();
        for (const QString &folder : folders) {
            QDirIterator it(folder, {QStringLiteral("*.desktop")}, QDir::Files, QDirIterator::Subdirectories | QDirIterator::FollowSymlinks);
            while (it.hasNext()) {
                const QString file = it.next();
                const QString id = QDir(folder).relativeFilePath(file).replace(u'/', u'-');
                // The earliest folder has the ID, even when its entry is hidden.
                if (seen.contains(id)) {
                    continue;
                }
                seen.insert(id);
                QFile entry(file);
                bool ok = false;
                if (entry.open(QIODevice::ReadOnly)) {
                    const App app = parseEntry(file, id, entry.readAll(), language, &ok);
                    if (ok) {
                        m_apps.insert(id, app);
                    }
                }
            }
        }
    }

    // The lists, most important first: the user's, the system's, then the programs' folders; in
    // each, the desktop's own before the shared one. Read each time: they are small.
    QStringList lists;
    const QStringList desktops = currentDesktops();
    const auto addFrom = [&](const QString &folder) {
        for (const QString &desktop : desktops) {
            lists.append(folder + u'/' + desktop.toLower() + QStringLiteral("-mimeapps.list"));
        }
        lists.append(folder + QStringLiteral("/mimeapps.list"));
    };
    for (const QString &folder : QStandardPaths::standardLocations(QStandardPaths::GenericConfigLocation)) {
        addFrom(folder);
    }
    for (const QString &folder : folders) {
        addFrom(folder);
    }
    m_associations = {};
    for (const QString &path : std::as_const(lists)) {
        QFile file(path);
        if (!file.open(QIODevice::ReadOnly)) {
            continue;
        }
        QString group;
        QHash<QString, QStringList> added;
        for (const QByteArray &raw : file.readAll().split('\n')) {
            const QString line = QString::fromUtf8(raw).trimmed();
            if (line.startsWith(u'[')) {
                group = line;
                continue;
            }
            const qsizetype equals = line.indexOf(u'=');
            if (equals <= 0 || line.startsWith(u'#')) {
                continue;
            }
            const QString type = line.left(equals).trimmed();
            const QStringList ids = list(line.mid(equals + 1));
            if (group == QLatin1String("[Default Applications]")) {
                m_associations.defaults[type] += ids;
            } else if (group == QLatin1String("[Added Associations]")) {
                added[type] += ids;
            } else if (group == QLatin1String("[Removed Associations]")) {
                m_associations.removed[type] += ids;
            }
        }
        // An association removed here or in a more important list stays removed.
        for (auto it = added.cbegin(); it != added.cend(); ++it) {
            for (const QString &id : it.value()) {
                if (!m_associations.removed.value(it.key()).contains(id)) {
                    m_associations.added[it.key()].append(id);
                }
            }
        }
    }
}

QStringList DesktopApps::mimeTypesOf(const QStringList &paths) const {
    QMimeDatabase database;
    QStringList types;
    for (const QString &path : paths) {
        const QString type = database.mimeTypeForFile(path).name();
        if (!types.contains(type)) {
            types.append(type);
        }
    }
    return types;
}

QPair<QString, QStringList> DesktopApps::handlers(const QString &type) const {
    QMimeDatabase database;
    const QMimeType mime = database.mimeTypeForName(type);
    // The type, then what it is a kind of (a C file is a text file).
    QStringList chain{mime.isValid() ? mime.name() : type};
    if (mime.isValid()) {
        chain += mime.aliases();
        chain += mime.allAncestors();
    }
    QString chosen;
    for (const QString &t : std::as_const(chain)) {
        for (const QString &id : m_associations.defaults.value(t)) {
            if (m_apps.contains(id)) {
                chosen = id;
                break;
            }
        }
        if (!chosen.isEmpty()) {
            break;
        }
    }
    QStringList rest;
    for (const QString &t : std::as_const(chain)) {
        const QStringList removed = m_associations.removed.value(t);
        for (const QString &id : m_associations.added.value(t)) {
            if (m_apps.contains(id) && id != chosen && !rest.contains(id)) {
                rest.append(id);
            }
        }
        QStringList claiming;
        for (const App &app : m_apps) {
            if (app.mimeTypes.contains(t) && !removed.contains(app.id) && app.id != chosen && !rest.contains(app.id)) {
                claiming.append(app.id);
            }
        }
        std::sort(claiming.begin(), claiming.end(), [this](const QString &a, const QString &b) {
            return QString::localeAwareCompare(m_apps.value(a).name, m_apps.value(b).name) < 0;
        });
        rest += claiming;
    }
    return {chosen, rest};
}

QVariantMap DesktopApps::entry(const App &app, bool isDefault) const {
    return {{QStringLiteral("id"), app.id},
            {QStringLiteral("name"), app.name},
            {QStringLiteral("icon"), app.icon},
            {QStringLiteral("isDefault"), isDefault}};
}

QVariantList DesktopApps::appsFor(const QStringList &paths, int max) {
    refresh();
    const QStringList types = mimeTypesOf(paths);
    if (types.isEmpty()) {
        return {};
    }
    // What opens them all: the first type's, kept where every other type has it too.
    auto [chosen, rest] = handlers(types.first());
    QStringList ids = chosen.isEmpty() ? rest : QStringList{chosen} + rest;
    for (qsizetype i = 1; i < types.size(); ++i) {
        const auto [otherDefault, otherRest] = handlers(types.at(i));
        const QStringList all = QStringList{otherDefault} + otherRest;
        ids.removeIf([&](const QString &id) { return !all.contains(id); });
        if (otherDefault != chosen) {
            chosen.clear();
        }
    }
    QVariantList result;
    for (const QString &id : std::as_const(ids)) {
        if (max >= 0 && result.size() >= max) {
            break;
        }
        result.append(entry(m_apps.value(id), id == chosen));
    }
    return result;
}

QVariantList DesktopApps::otherApps(const QStringList &paths) {
    QSet<QString> offered;
    for (const QVariant &app : appsFor(paths)) {
        offered.insert(app.toMap().value(QStringLiteral("id")).toString());
    }
    QList<App> others;
    for (const App &app : std::as_const(m_apps)) {
        if (!app.noDisplay && !offered.contains(app.id)) {
            others.append(app);
        }
    }
    std::sort(others.begin(), others.end(), [](const App &a, const App &b) { return QString::localeAwareCompare(a.name, b.name) < 0; });
    QVariantList result;
    for (const App &app : std::as_const(others)) {
        result.append(entry(app, false));
    }
    return result;
}

QString DesktopApps::launch(const QString &id, const QStringList &paths) {
    refresh();
    const auto found = m_apps.constFind(id);
    if (found == m_apps.cend()) {
        return tr("That program is no longer installed.");
    }
    const QList<QStringList> lines = commandLines(*found, paths);
    if (lines.isEmpty()) {
        return found->terminal ? tr("No terminal was found to run %1 in.").arg(found->name)
                               : tr("%1 can't be started: its command is not readable.").arg(found->name);
    }
    for (const QStringList &line : lines) {
        if (!QProcess::startDetached(line.first(), line.mid(1), found->path)) {
            return tr("%1 could not be started.").arg(found->name);
        }
    }
    return {};
}

QString DesktopApps::withDefault(const QString &contents, const QString &id, const QStringList &types) {
    QStringList lines = contents.split(u'\n');
    if (!lines.isEmpty() && lines.last().isEmpty()) {
        lines.removeLast();
    }
    // Sets `type=…` in `group`, the group added at the end when there is none.
    const auto set = [&lines](const QString &group, const QString &type, const auto &value) {
        qsizetype start = lines.indexOf(group);
        if (start < 0) {
            if (!lines.isEmpty() && !lines.last().trimmed().isEmpty()) {
                lines.append(QString());
            }
            lines.append(group);
            start = lines.size() - 1;
        }
        qsizetype end = start + 1;
        while (end < lines.size() && !lines.at(end).trimmed().startsWith(u'[')) {
            ++end;
        }
        for (qsizetype i = start + 1; i < end; ++i) {
            const qsizetype equals = lines.at(i).indexOf(u'=');
            if (equals > 0 && lines.at(i).left(equals).trimmed() == type) {
                lines[i] = type + u'=' + value(list(lines.at(i).mid(equals + 1))).join(u';') + u';';
                return;
            }
        }
        // After the group's last entry, before any blank lines that end it.
        qsizetype at = end;
        while (at > start + 1 && lines.at(at - 1).trimmed().isEmpty()) {
            --at;
        }
        lines.insert(at, type + u'=' + value(QStringList()).join(u';') + u';');
    };
    for (const QString &type : types) {
        set(QStringLiteral("[Default Applications]"), type, [&](const QStringList &) { return QStringList{id}; });
        set(QStringLiteral("[Added Associations]"), type, [&](QStringList ids) {
            ids.removeAll(id);
            ids.prepend(id);
            return ids;
        });
    }
    return lines.join(u'\n') + u'\n';
}

bool DesktopApps::setDefault(const QString &id, const QStringList &paths) {
    refresh();
    const QStringList types = mimeTypesOf(paths);
    if (!m_apps.contains(id) || types.isEmpty()) {
        return false;
    }
    QFile current(userListPath());
    const QString contents = current.open(QIODevice::ReadOnly) ? QString::fromUtf8(current.readAll()) : QString();
    current.close();
    QDir().mkpath(QFileInfo(userListPath()).path());
    QSaveFile file(userListPath());
    if (!file.open(QIODevice::WriteOnly)) {
        return false;
    }
    file.write(withDefault(contents, id, types).toUtf8());
    return file.commit();
}

QString DesktopApps::typesText(const QStringList &paths) const {
    QMimeDatabase database;
    QStringList texts;
    for (const QString &type : mimeTypesOf(paths)) {
        const QString comment = database.mimeTypeForName(type).comment();
        if (!texts.contains(comment)) {
            texts.append(comment);
        }
    }
    return texts.join(QStringLiteral(", "));
}
