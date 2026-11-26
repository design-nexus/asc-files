// SPDX-License-Identifier: MIT
#include "FolderPicker.h"

#include "Terms.h"
#include "Visits.h"

#include <QDir>
#include <QDirIterator>
#include <QCoreApplication>
#include <QFileInfo>
#include <QPointer>
#include <QThreadPool>

FolderPicker::FolderPicker(Visits *visits, QObject *parent) : QObject(parent), m_visits(visits) {}

FolderPicker::~FolderPicker() { cancel(); }

FolderPicker::Where FolderPicker::where(const QString &text, const QString &open) {
    const bool pathLike = text.startsWith(u'/') || text == QLatin1String("~") || text.startsWith(QLatin1String("~/")) ||
                          text.startsWith(QLatin1String("./")) || text.startsWith(QLatin1String("../")) || text == QLatin1String("..");
    if (text.startsWith(u'~') && !pathLike) {
        return {{}, {}, QObject::tr("Only ~ and ~/ are supported"), false};
    }
    if (!pathLike) {
        if (open.isEmpty()) {
            return {{}, {}, QObject::tr("Type a full path here"), false};
        }
        return {open, text, {}, false};
    }
    QString path = text == QLatin1String("~") ? QStringLiteral("~/") : text == QLatin1String("..") ? QStringLiteral("../") : text;
    if (path.startsWith(u'~')) {
        path = QDir::homePath() + path.mid(1);
    } else if (!path.startsWith(u'/')) {
        if (open.isEmpty()) {
            return {{}, {}, QObject::tr("Type a full path here"), false};
        }
        path = open + u'/' + path;
    }
    const qsizetype slash = path.lastIndexOf(u'/');
    // `.` and `..` resolve as text.
    const QString base = QDir::cleanPath(path.left(slash + 1));
    const QString query = path.mid(slash + 1);
    return {base.isEmpty() ? QStringLiteral("/") : base, query, {}, query.isEmpty()};
}

QStringList FolderPicker::find(const Where &where, bool hidden, const QHash<QString, double> &frecencies, const std::atomic_bool &cancelled) {
    const Terms terms(where.query);
    const QString typed = where.query.trimmed();
    // Hidden folders when the listing shows them, or a term starts with a dot.
    const bool showHidden = hidden || where.query.startsWith(u'.') || where.query.contains(QLatin1String(" ."));
    struct Found {
        QString path;
        int rank;
        int inName;
        int loose;
        double frecency;
        int depth;
    };
    QList<Found> found;
    const QString base = where.base;
    const QString exact = typed.isEmpty() ? QString() : QDir::cleanPath(base + u'/' + typed);
    QDir::Filters filters = QDir::Dirs | QDir::NoDotAndDotDot | QDir::NoSymLinks;
    if (showHidden) {
        filters |= QDir::Hidden;
    }
    QDirIterator it(base, filters, QDirIterator::Subdirectories);
    int walked = 0;
    while (it.hasNext() && walked < Walked && !cancelled) {
        const QString path = it.next();
        ++walked;
        if (base == QLatin1String("/") &&
            (path.startsWith(QLatin1String("/proc/")) || path.startsWith(QLatin1String("/sys/")) || path.startsWith(QLatin1String("/dev/")) ||
             path == QLatin1String("/proc") || path == QLatin1String("/sys") || path == QLatin1String("/dev"))) {
            continue;
        }
        const QString relative = path.mid(base == QLatin1String("/") ? 1 : base.size() + 1);
        if (!terms.matches(relative)) {
            continue;
        }
        const QString name = it.fileName();
        // What the whole query names outright first: that path, then that name.
        const int rank = path == exact ? 2 : (!typed.isEmpty() && name.compare(typed, Qt::CaseInsensitive) == 0) ? 1 : 0;
        found.append({path, rank, terms.inName(name), terms.looseness(name), frecencies.value(path), int(relative.count(u'/'))});
    }
    std::sort(found.begin(), found.end(), [](const Found &a, const Found &b) {
        if (a.rank != b.rank) {
            return a.rank > b.rank;
        }
        if (a.inName != b.inName) {
            return a.inName > b.inName;
        }
        if (a.loose != b.loose) {
            return a.loose < b.loose;
        }
        if (a.frecency != b.frecency) {
            return a.frecency > b.frecency;
        }
        if (a.depth != b.depth) {
            return a.depth < b.depth;
        }
        return a.path < b.path;
    });
    QStringList folders;
    if (where.itself) {
        folders.append(base);
    }
    for (const Found &one : std::as_const(found)) {
        if (folders.size() >= Listed) {
            break;
        }
        folders.append(one.path);
    }
    return folders;
}

QString FolderPicker::resolve(const QString &text, const QString &open) {
    QString path = text.trimmed();
    if (path == QLatin1String("~") || path.startsWith(QLatin1String("~/"))) {
        path = QDir::homePath() + path.mid(1);
    } else if (!path.startsWith(u'/')) {
        if (open.isEmpty()) {
            return {};
        }
        path = open + u'/' + path;
    }
    return QDir::cleanPath(path);
}

QString FolderPicker::kind(const QString &path) {
    const QFileInfo info(path);
    return info.isDir() ? QStringLiteral("folder") : info.exists() || info.isSymLink() ? QStringLiteral("file") : QString();
}

void FolderPicker::cancel() {
    if (m_cancelled) {
        *m_cancelled = true;
        m_cancelled.reset();
    }
    if (m_searching) {
        m_searching = false;
        emit changed();
    }
}

void FolderPicker::search(const QString &text, const QString &open, bool hidden) {
    cancel();
    const Where where = FolderPicker::where(text, open);
    m_folders.clear();
    m_message = where.error;
    if (!where.error.isEmpty() || !QFileInfo(where.base).isDir()) {
        if (m_message.isEmpty()) {
            m_message = tr("No such folder");
        }
        emit changed();
        return;
    }
    m_searching = true;
    emit changed();
    auto cancelled = std::make_shared<std::atomic_bool>(false);
    m_cancelled = cancelled;
    QPointer<FolderPicker> self(this);
    // The visits as they are now: the search runs on another thread.
    const QHash<QString, double> frecencies = m_visits ? m_visits->frecencies() : QHash<QString, double>();
    QThreadPool::globalInstance()->start([self, where, hidden, frecencies, cancelled] {
        const QStringList folders = find(where, hidden, frecencies, *cancelled);
        if (*cancelled) {
            return;
        }
        // Answered on the UI thread, if the picker is still there and still wants it.
        QMetaObject::invokeMethod(
            QCoreApplication::instance(),
            [self, folders, cancelled] {
                if (!self || *cancelled) {
                    return;
                }
                self->m_folders = folders;
                self->m_searching = false;
                self->m_message = folders.isEmpty() ? tr("No matching folders") : QString();
                emit self->changed();
            },
            Qt::QueuedConnection);
    });
}
