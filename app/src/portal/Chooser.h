// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QHash>
#include <QList>
#include <QPair>
#include <QSharedPointer>
#include <QString>
#include <atomic>
#include <chrono>
#include <memory>
#include <optional>
#include <variant>

/// The file chooser Files offers the desktop (`org.freedesktop.impl.portal.FileChooser`): what a
/// request asks for, checked as legacy's portal checked it (every input from another program is
/// bounded), and the answers it gives. Paths are raw bytes, so no name is lost on the way.
namespace Portal {

inline constexpr int FileChooserVersion = 4;
inline constexpr int MaxActiveRequests = 16;
inline constexpr int MaxChoices = 16;
inline constexpr int MaxChoiceOptions = 32;
inline constexpr int MaxTotalChoiceOptions = 128;
inline constexpr int MaxGlobBytes = 256;
inline constexpr int MaxGlobStarRuns = 2;
inline constexpr int MaxSaveFiles = 256;
inline constexpr int MaxStringBytes = 4096;
inline constexpr int MaxFilenameBytes = 255;
inline constexpr std::chrono::seconds IdleTimeout{120};

/// A filter's rule: a glob (`0`) or a MIME type (`1`), as the portal sends them.
struct Rule {
    uint type = 0;
    QString pattern;
    bool operator==(const Rule &) const = default;
};

struct Filter {
    QString label;
    QList<Rule> rules;
    bool operator==(const Filter &) const = default;
};

/// A choice the caller adds to the chooser: a check box when it has no options, a list otherwise.
struct Choice {
    QString id;
    QString label;
    QList<QPair<QString, QString>> options;
    QString initial;
    bool operator==(const Choice &) const = default;
};

/// What a request's caller asked, before it is checked.
struct Options {
    std::optional<QString> acceptLabel;
    std::optional<bool> modal;
    std::optional<bool> multiple;
    std::optional<bool> directory;
    QList<Filter> filters;
    std::optional<Filter> currentFilter;
    QList<Choice> choices;
    std::optional<QByteArray> currentFolder;
    std::optional<QByteArray> currentFile;
    std::optional<QString> currentName;
    /// SaveFiles: the names to save.
    QList<QByteArray> files;
};

enum class Kind { Open, SaveFile, SaveFiles };

/// A request, checked and resolved: what the chooser window shows.
struct Request {
    Kind kind = Kind::Open;
    QString title;
    QString acceptLabel;
    bool modal = true;
    bool directory = false;
    bool multiple = false;
    /// Where it opens: the caller's folder where it can be read, else Downloads.
    QByteArray folder;
    /// SaveFile: the name offered; SaveFiles: the names saved.
    std::optional<QByteArray> currentName;
    QList<QByteArray> names;
    QList<Filter> filters;
    std::optional<Filter> currentFilter;
    QList<Choice> choices;
};

/// A request refused: its reason, answered as `org.freedesktop.portal.Error.InvalidArgument`.
using Checked = std::variant<Request, QString>;

Checked openRequest(const QString &title, const Options &options);
Checked saveFileRequest(const QString &title, const Options &options);
Checked saveFilesRequest(const QString &title, const Options &options);

std::optional<QString> validateString(const QString &value, int limit, const QString &field);
std::optional<QString> validatePath(const std::optional<QByteArray> &path, const QString &field);
std::optional<QString> validateGlob(const QString &pattern);
std::optional<QString> validateFilters(const QList<Filter> &filters, const std::optional<Filter> &current);
std::optional<QString> validateChoices(const QList<Choice> &choices);
std::optional<QString> validateCommon(const QString &title, const std::optional<QString> &acceptLabel, const QList<Choice> &choices);
std::optional<QString> validateSaveNames(const QList<QByteArray> &names);
/// A name a file can be saved under in a folder: not empty, `.` or `..`, no `/` or NUL.
bool safeFilename(const QByteArray &name);

/// Downloads, else the home folder.
QByteArray defaultSaveFolder();
/// `hint` where it is an absolute folder that can be read, else the default.
QByteArray accessibleFolder(const std::optional<QByteArray> &hint);
/// Where a SaveFile chooser opens and the name it offers. A current file (an existing regular
/// file, or one not yet made in a readable folder) wins over the folder and name; one that won't
/// do falls back to the default with no name, never to the lower hints.
QPair<QByteArray, std::optional<QByteArray>> saveFileSuggestion(const std::optional<QByteArray> &currentFile,
                                                               const std::optional<QByteArray> &currentFolder,
                                                               const std::optional<QString> &currentName);

/// `file://` with the path's bytes percent-encoded; empty for a path that isn't absolute.
QString localUri(const QByteArray &path);

/// One item chosen in an Open chooser.
struct Chosen {
    QByteArray path;
    bool folder = false;
    /// A local item (remote ones can't be answered).
    bool local = true;
};
/// The paths an Open chooser answers with, or why the choice won't do: kind (folders or files),
/// how many, and only local items. A folder chooser with nothing chosen answers the folder shown.
std::variant<QList<QByteArray>, QString> openSelection(const QList<Chosen> &chosen, const QByteArray &current, bool directory,
                                                       bool multiple);

/// Where SaveFiles would write: each name in `folder`, and whether any is a file already
/// (the window asks before replacing). A folder of a name refuses.
struct Destinations {
    QList<QByteArray> paths;
    bool existingFiles = false;
};
std::variant<Destinations, QString> checkDestinations(const QByteArray &folder, const QList<QByteArray> &names);

/// The requests in flight: each by its handle, cancellable before and after its window shows,
/// bounded in number; the chooser process ends once it has been idle long enough.
class RequestTracker {
public:
    using Clock = std::chrono::steady_clock;
    /// A request in flight; it ends when the last copy goes.
    class Tracked {
    public:
        bool cancelled() const { return m_cancelled->load(); }
        QString token() const { return m_token; }

    private:
        friend class RequestTracker;
        QString m_token;
        QSharedPointer<std::atomic_bool> m_cancelled;
        std::shared_ptr<void> m_end;
    };

    /// A request under `token`; none while one of the token is in flight, past the limit, or once
    /// shutting down.
    std::optional<Tracked> begin(const QString &token);
    /// Cancels the request in flight under `token`; false when there is none.
    bool cancel(const QString &token);
    /// Starts shutting down when nothing is in flight and nothing happened for `IdleTimeout`.
    bool beginShutdownIfIdle(Clock::time_point now);
    /// For tests: when something last happened.
    void setLastActivity(Clock::time_point when) { m_lastActivity = when; }

private:
    void end(const QString &token, const QSharedPointer<std::atomic_bool> &flag);
    QHash<QString, QSharedPointer<std::atomic_bool>> m_active;
    Clock::time_point m_lastActivity = Clock::now();
    bool m_shuttingDown = false;
};

} // namespace Portal
