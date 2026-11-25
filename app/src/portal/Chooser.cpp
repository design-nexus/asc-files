// SPDX-License-Identifier: MIT
#include "Chooser.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QStandardPaths>
#include <QUrl>
#include <cerrno>
#include <cstring>
#include <dirent.h>
#include <sys/stat.h>

namespace Portal {

namespace {

QString tr(const char *text) { return QCoreApplication::translate("Portal", text); }

/// The caller's title as it is; only Files' own is translated.
QString requestTitle(const QString &title, const char *fallback) { return title.trimmed().isEmpty() ? tr(fallback) : title; }

bool directoryIsAccessible(const QByteArray &path) {
    DIR *dir = ::opendir(path.constData());
    if (!dir) {
        return false;
    }
    ::closedir(dir);
    return true;
}

QByteArray parentOf(const QByteArray &path) {
    const qsizetype slash = path.lastIndexOf('/');
    return slash <= 0 ? QByteArray("/") : path.left(slash);
}

QByteArray nameOf(const QByteArray &path) { return path.mid(path.lastIndexOf('/') + 1); }

QByteArray join(const QByteArray &folder, const QByteArray &name) { return folder.endsWith('/') ? folder + name : folder + '/' + name; }

} // namespace

std::optional<QString> validateString(const QString &value, int limit, const QString &field) {
    if (value.toUtf8().size() > limit || value.contains(QChar(0))) {
        return tr("%1 is too long or contains a NUL byte").arg(field);
    }
    return std::nullopt;
}

std::optional<QString> validatePath(const std::optional<QByteArray> &path, const QString &field) {
    if (path && (path->size() > MaxStringBytes || path->contains('\0'))) {
        return tr("%1 is too long or contains a NUL byte").arg(field);
    }
    return std::nullopt;
}

std::optional<QString> validateGlob(const QString &pattern) {
    if (auto error = validateString(pattern, MaxGlobBytes, QStringLiteral("glob filter rule"))) {
        return error;
    }
    // Patterns with many wildcard groups take a matcher a long time; two will do for a chooser.
    int runs = 0;
    bool previousStar = false;
    for (const QChar c : pattern) {
        if (c == u'*' && !previousStar && ++runs > MaxGlobStarRuns) {
            return tr("glob filter rule has too many wildcard groups");
        }
        previousStar = c == u'*';
    }
    return std::nullopt;
}

std::optional<QString> validateFilters(const QList<Filter> &filters, const std::optional<Filter> &current) {
    QList<Filter> all = filters;
    if (current && !filters.contains(*current)) {
        all.append(*current);
    }
    // Long lists are accepted as they are (sites list hundreds of upload types).
    for (const Filter &filter : all) {
        if (auto error = validateString(filter.label, MaxStringBytes, QStringLiteral("file filter label"))) {
            return error;
        }
        for (const Rule &rule : filter.rules) {
            if (auto error = rule.type == 0 ? validateGlob(rule.pattern) : validateString(rule.pattern, MaxStringBytes, QStringLiteral("MIME filter rule"))) {
                return error;
            }
        }
    }
    return std::nullopt;
}

std::optional<QString> validateChoices(const QList<Choice> &choices) {
    if (choices.size() > MaxChoices) {
        return tr("too many file chooser choices");
    }
    int total = 0;
    for (const Choice &choice : choices) {
        for (const auto &[value, field] : {QPair{choice.id, QStringLiteral("choice identifier")}, QPair{choice.label, QStringLiteral("choice label")},
                                           QPair{choice.initial, QStringLiteral("initial choice value")}}) {
            if (auto error = validateString(value, MaxStringBytes, field)) {
                return error;
            }
        }
        if (choice.options.size() > MaxChoiceOptions) {
            return tr("a file chooser choice has too many options");
        }
        total += int(choice.options.size());
        if (total > MaxTotalChoiceOptions) {
            return tr("file chooser choices have too many options");
        }
        for (const auto &[id, label] : choice.options) {
            if (auto error = validateString(id, MaxStringBytes, QStringLiteral("choice option identifier"))) {
                return error;
            }
            if (auto error = validateString(label, MaxStringBytes, QStringLiteral("choice option label"))) {
                return error;
            }
        }
    }
    return std::nullopt;
}

std::optional<QString> validateCommon(const QString &title, const std::optional<QString> &acceptLabel, const QList<Choice> &choices) {
    if (auto error = validateString(title, MaxStringBytes, QStringLiteral("title"))) {
        return error;
    }
    if (acceptLabel) {
        if (auto error = validateString(*acceptLabel, MaxStringBytes, QStringLiteral("accept label"))) {
            return error;
        }
    }
    return validateChoices(choices);
}

bool safeFilename(const QByteArray &name) {
    return !name.isEmpty() && !name.contains('/') && !name.contains('\0') && name != "." && name != "..";
}

std::optional<QString> validateSaveNames(const QList<QByteArray> &names) {
    if (names.isEmpty() || names.size() > MaxSaveFiles) {
        return tr("SaveFiles requires a bounded, non-empty filename list");
    }
    for (const QByteArray &name : names) {
        if (!safeFilename(name) || name.size() > MaxFilenameBytes) {
            return tr("SaveFiles filenames must be bounded safe basenames");
        }
    }
    return std::nullopt;
}

QByteArray defaultSaveFolder() {
    const QString downloads = QStandardPaths::writableLocation(QStandardPaths::DownloadLocation);
    return QFile::encodeName(downloads.isEmpty() ? QDir::homePath() : downloads);
}

QByteArray accessibleFolder(const std::optional<QByteArray> &hint) {
    if (hint && hint->startsWith('/') && directoryIsAccessible(*hint)) {
        return *hint;
    }
    return defaultSaveFolder();
}

QPair<QByteArray, std::optional<QByteArray>> saveFileSuggestion(const std::optional<QByteArray> &currentFile,
                                                               const std::optional<QByteArray> &currentFolder,
                                                               const std::optional<QString> &currentName) {
    if (currentFile) {
        struct stat st {};
        // Qt also names a file not made yet: only its folder needs to be there.
        const bool target = currentFile->startsWith('/') &&
                            (::stat(currentFile->constData(), &st) == 0 ? S_ISREG(st.st_mode) : errno == ENOENT);
        const QByteArray name = nameOf(*currentFile);
        if (target && safeFilename(name) && directoryIsAccessible(parentOf(*currentFile))) {
            return {parentOf(*currentFile), name};
        }
        return {defaultSaveFolder(), std::nullopt};
    }
    std::optional<QByteArray> name;
    if (currentName && safeFilename(currentName->toUtf8()) && currentName->toUtf8().size() <= MaxFilenameBytes) {
        name = currentName->toUtf8();
    }
    return {accessibleFolder(currentFolder), name};
}

QString localUri(const QByteArray &path) {
    if (!path.startsWith('/')) {
        return {};
    }
    // The bytes themselves: a name that isn't UTF-8 keeps them.
    return QStringLiteral("file://") + QString::fromLatin1(path.toPercentEncoding("/"));
}

std::variant<QList<QByteArray>, QString> openSelection(const QList<Chosen> &chosen, const QByteArray &current, bool directory,
                                                       bool multiple) {
    if (chosen.isEmpty() && directory) {
        if (current.startsWith('/')) {
            return QList<QByteArray>{current};
        }
        return tr("Choose a local folder");
    }
    if (chosen.isEmpty()) {
        return tr("Choose a file");
    }
    if (!multiple && chosen.size() != 1) {
        return tr("Choose one item");
    }
    QList<QByteArray> paths;
    for (const Chosen &one : chosen) {
        if (one.folder != directory) {
            return directory ? tr("Choose folders only") : tr("Choose files only");
        }
        if (!one.local) {
            return tr("Choose local items only");
        }
        paths.append(one.path);
    }
    return paths;
}

std::variant<Destinations, QString> checkDestinations(const QByteArray &folder, const QList<QByteArray> &names) {
    if (!folder.startsWith('/') || folder.size() > MaxStringBytes || !directoryIsAccessible(folder)) {
        return tr("Choose an accessible local folder");
    }
    if (validateSaveNames(names)) {
        return tr("Enter bounded, safe filenames");
    }
    Destinations destinations;
    for (const QByteArray &name : names) {
        const QByteArray path = join(folder, name);
        struct stat st {};
        if (::stat(path.constData(), &st) == 0) {
            // A link to a folder counts as the folder it leads to.
            if (S_ISDIR(st.st_mode)) {
                return tr("A folder named “%1” already exists").arg(QFile::decodeName(name));
            }
            destinations.existingFiles = true;
        } else if (::lstat(path.constData(), &st) == 0) {
            destinations.existingFiles = true;
        } else if (errno != ENOENT) {
            return tr("Unable to inspect the destination: %1").arg(QString::fromLocal8Bit(strerror(errno)));
        }
        destinations.paths.append(path);
    }
    return destinations;
}

Checked openRequest(const QString &title, const Options &options) {
    if (auto error = validateCommon(title, options.acceptLabel, options.choices)) {
        return *error;
    }
    if (auto error = validateFilters(options.filters, options.currentFilter)) {
        return *error;
    }
    if (auto error = validatePath(options.currentFolder, QStringLiteral("current folder"))) {
        return *error;
    }
    Request request;
    request.kind = Kind::Open;
    request.title = requestTitle(title, "Open Files");
    request.acceptLabel = options.acceptLabel.value_or(tr("Open"));
    request.modal = options.modal.value_or(true);
    request.directory = options.directory.value_or(false);
    request.multiple = options.multiple.value_or(false);
    request.folder = accessibleFolder(options.currentFolder);
    request.filters = options.filters;
    request.currentFilter = options.currentFilter;
    request.choices = options.choices;
    return request;
}

Checked saveFileRequest(const QString &title, const Options &options) {
    if (auto error = validateCommon(title, options.acceptLabel, options.choices)) {
        return *error;
    }
    if (auto error = validateFilters(options.filters, options.currentFilter)) {
        return *error;
    }
    if (auto error = validatePath(options.currentFile, QStringLiteral("current file"))) {
        return *error;
    }
    if (auto error = validatePath(options.currentFolder, QStringLiteral("current folder"))) {
        return *error;
    }
    if (options.currentName) {
        if (auto error = validateString(*options.currentName, MaxFilenameBytes, QStringLiteral("current filename"))) {
            return *error;
        }
    }
    Request request;
    request.kind = Kind::SaveFile;
    request.title = requestTitle(title, "Save File");
    request.acceptLabel = options.acceptLabel.value_or(tr("Save"));
    request.modal = options.modal.value_or(true);
    const auto [folder, name] = saveFileSuggestion(options.currentFile, options.currentFolder, options.currentName);
    request.folder = folder;
    request.currentName = name;
    request.filters = options.filters;
    request.currentFilter = options.currentFilter;
    request.choices = options.choices;
    return request;
}

Checked saveFilesRequest(const QString &title, const Options &options) {
    if (auto error = validateCommon(title, options.acceptLabel, options.choices)) {
        return *error;
    }
    if (auto error = validatePath(options.currentFolder, QStringLiteral("current folder"))) {
        return *error;
    }
    if (auto error = validateSaveNames(options.files)) {
        return *error;
    }
    Request request;
    request.kind = Kind::SaveFiles;
    request.title = requestTitle(title, "Save Files");
    request.acceptLabel = options.acceptLabel.value_or(tr("Save"));
    request.modal = options.modal.value_or(true);
    request.folder = accessibleFolder(options.currentFolder);
    request.names = options.files;
    request.choices = options.choices;
    return request;
}

std::optional<RequestTracker::Tracked> RequestTracker::begin(const QString &token) {
    if (m_shuttingDown || m_active.contains(token) || m_active.size() >= MaxActiveRequests) {
        return std::nullopt;
    }
    m_lastActivity = Clock::now();
    Tracked tracked;
    tracked.m_token = token;
    tracked.m_cancelled = QSharedPointer<std::atomic_bool>::create(false);
    m_active.insert(token, tracked.m_cancelled);
    const QSharedPointer<std::atomic_bool> flag = tracked.m_cancelled;
    tracked.m_end = std::shared_ptr<void>(nullptr, [this, token, flag](void *) { end(token, flag); });
    return tracked;
}

bool RequestTracker::cancel(const QString &token) {
    const auto found = m_active.constFind(token);
    if (found == m_active.constEnd()) {
        return false;
    }
    found.value()->store(true);
    return true;
}

void RequestTracker::end(const QString &token, const QSharedPointer<std::atomic_bool> &flag) {
    // Only this request's entry: a later one may hold the token already.
    if (m_active.value(token) == flag) {
        m_active.remove(token);
    }
    m_lastActivity = Clock::now();
}

bool RequestTracker::beginShutdownIfIdle(Clock::time_point now) {
    if (m_shuttingDown || !m_active.isEmpty() || now - m_lastActivity < IdleTimeout) {
        return m_shuttingDown;
    }
    m_shuttingDown = true;
    return true;
}

} // namespace Portal
