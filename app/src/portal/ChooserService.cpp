// SPDX-License-Identifier: MIT
#include "ChooserService.h"

#include "Placement.h"

#include <QDBusConnection>
#include <QDBusMetaType>
#include <QFile>
#include <QFileInfo>
#include <QMimeDatabase>

// The portal's types on the wire: a rule `(us)`, a filter `(sa(us))`, a choice `(ssa(ss)s)`, a
// chosen value `(ss)`.
QDBusArgument &operator<<(QDBusArgument &argument, const Portal::ChoiceValue &value);
const QDBusArgument &operator>>(const QDBusArgument &argument, Portal::ChoiceValue &value);

QDBusArgument &operator<<(QDBusArgument &argument, const Portal::Rule &rule) {
    argument.beginStructure();
    argument << rule.type << rule.pattern;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, Portal::Rule &rule) {
    argument.beginStructure();
    argument >> rule.type >> rule.pattern;
    argument.endStructure();
    return argument;
}

QDBusArgument &operator<<(QDBusArgument &argument, const Portal::Filter &filter) {
    argument.beginStructure();
    argument << filter.label << filter.rules;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, Portal::Filter &filter) {
    argument.beginStructure();
    argument >> filter.label >> filter.rules;
    argument.endStructure();
    return argument;
}

QDBusArgument &operator<<(QDBusArgument &argument, const Portal::Choice &choice) {
    argument.beginStructure();
    argument << choice.id << choice.label;
    argument.beginArray(qMetaTypeId<Portal::ChoiceValue>());
    for (const auto &[id, label] : choice.options) {
        argument << Portal::ChoiceValue{id, label};
    }
    argument.endArray();
    argument << choice.initial;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, Portal::Choice &choice) {
    argument.beginStructure();
    argument >> choice.id >> choice.label;
    choice.options.clear();
    argument.beginArray();
    while (!argument.atEnd()) {
        Portal::ChoiceValue option;
        argument >> option;
        choice.options.append({option.id, option.value});
    }
    argument.endArray();
    argument >> choice.initial;
    argument.endStructure();
    return argument;
}

QDBusArgument &operator<<(QDBusArgument &argument, const Portal::ChoiceValue &value) {
    argument.beginStructure();
    argument << value.id << value.value;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, Portal::ChoiceValue &value) {
    argument.beginStructure();
    argument >> value.id >> value.value;
    argument.endStructure();
    return argument;
}

namespace Portal {

namespace {

/// The Request object a call's handle names: the caller closes the chooser through it.
class RequestObject : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.impl.portal.Request")
public:
    RequestObject(ChooserJob *job) : QObject(job), m_job(job) {}
public slots:
    Q_SCRIPTABLE void Close() { m_job->cancel(); }

private:
    ChooserJob *m_job;
};

/// A value as `T`, whether it came over the bus (a `QDBusArgument`) or as `T` already.
template <typename T> std::optional<T> take(const QVariantMap &options, const char *key) {
    const auto found = options.constFind(QLatin1String(key));
    if (found == options.constEnd()) {
        return std::nullopt;
    }
    return qdbus_cast<T>(found.value());
}

/// A path the portal sends as `ay`, with its trailing NUL.
std::optional<QByteArray> path(const QVariantMap &options, const char *key) {
    auto bytes = take<QByteArray>(options, key);
    if (bytes && bytes->endsWith('\0')) {
        bytes->chop(1);
    }
    return bytes;
}

QStringList globsOf(const Filter &filter) {
    static QMimeDatabase mimes;
    QStringList globs;
    for (const Rule &rule : filter.rules) {
        if (rule.type == 0) {
            globs.append(rule.pattern);
        } else {
            // A MIME type lists as its globs (and its subtypes' are its own here, as they would be).
            const QMimeType type = mimes.mimeTypeForName(rule.pattern);
            globs.append(type.isValid() ? type.globPatterns() : QStringList{});
            if (rule.pattern.endsWith(QLatin1String("/*"))) {
                const QString group = rule.pattern.chopped(1);
                for (const QMimeType &each : mimes.allMimeTypes()) {
                    if (each.name().startsWith(group)) {
                        globs.append(each.globPatterns());
                    }
                }
            }
        }
    }
    globs.removeDuplicates();
    // A filter naming nothing listable still lists something: everything.
    return globs.isEmpty() ? QStringList{QStringLiteral("*")} : globs;
}

} // namespace

void registerTypes() {
    qDBusRegisterMetaType<Rule>();
    qDBusRegisterMetaType<QList<Rule>>();
    qDBusRegisterMetaType<Filter>();
    qDBusRegisterMetaType<QList<Filter>>();
    qDBusRegisterMetaType<ChoiceValue>();
    qDBusRegisterMetaType<QList<ChoiceValue>>();
    qDBusRegisterMetaType<Choice>();
    qDBusRegisterMetaType<QList<Choice>>();
    qDBusRegisterMetaType<QList<QByteArray>>();
}

Options optionsFrom(const QVariantMap &options) {
    Options read;
    read.acceptLabel = take<QString>(options, "accept_label");
    read.modal = take<bool>(options, "modal");
    read.multiple = take<bool>(options, "multiple");
    read.directory = take<bool>(options, "directory");
    read.filters = take<QList<Filter>>(options, "filters").value_or(QList<Filter>{});
    read.currentFilter = take<Filter>(options, "current_filter");
    read.choices = take<QList<Choice>>(options, "choices").value_or(QList<Choice>{});
    read.currentFolder = path(options, "current_folder");
    read.currentFile = path(options, "current_file");
    read.currentName = take<QString>(options, "current_name");
    for (QByteArray file : take<QList<QByteArray>>(options, "files").value_or(QList<QByteArray>{})) {
        if (file.endsWith('\0')) {
            file.chop(1);
        }
        read.files.append(file);
    }
    return read;
}

ChooserJob::ChooserJob(Request request, QObject *parent) : QObject(parent), m_request(std::move(request)) {}

QString ChooserJob::kind() const {
    switch (m_request.kind) {
    case Kind::Open:
        return QStringLiteral("open");
    case Kind::SaveFile:
        return QStringLiteral("saveFile");
    case Kind::SaveFiles:
        return QStringLiteral("saveFiles");
    }
    return {};
}

QString ChooserJob::folder() const { return QFile::decodeName(m_request.folder); }

QString ChooserJob::currentName() const { return m_request.currentName ? QFile::decodeName(*m_request.currentName) : QString(); }

QStringList ChooserJob::names() const {
    QStringList names;
    for (const QByteArray &name : m_request.names) {
        names.append(QFile::decodeName(name));
    }
    return names;
}

QVariantList ChooserJob::filters() const {
    QVariantList filters;
    for (const Filter &filter : m_request.filters) {
        filters.append(QVariantMap{{"label", filter.label}, {"globs", globsOf(filter)}});
    }
    // A current filter not among the filters is offered too.
    if (m_request.currentFilter && !m_request.filters.contains(*m_request.currentFilter)) {
        filters.append(QVariantMap{{"label", m_request.currentFilter->label}, {"globs", globsOf(*m_request.currentFilter)}});
    }
    return filters;
}

int ChooserJob::currentFilter() const {
    if (!m_request.currentFilter) {
        return 0;
    }
    const qsizetype at = m_request.filters.indexOf(*m_request.currentFilter);
    return at >= 0 ? int(at) : int(m_request.filters.size());
}

QVariantList ChooserJob::choices() const {
    QVariantList choices;
    for (const Choice &choice : m_request.choices) {
        QVariantList options;
        for (const auto &[id, label] : choice.options) {
            options.append(QVariantMap{{"id", id}, {"label", label}});
        }
        choices.append(QVariantMap{{"id", choice.id}, {"label", choice.label}, {"options", options}, {"initial", choice.initial}});
    }
    return choices;
}

QString ChooserJob::acceptOpen(const QStringList &paths, const QString &current, const QVariantMap &choices, int filter) {
    QList<Chosen> chosen;
    for (const QString &path : paths) {
        chosen.append({QFile::encodeName(path), QFileInfo(path).isDir(), path.startsWith(u'/')});
    }
    const auto selection = openSelection(chosen, QFile::encodeName(current), m_request.directory, m_request.multiple);
    if (const QString *why = std::get_if<QString>(&selection)) {
        return *why;
    }
    answer(std::get<QList<QByteArray>>(selection), choices, filter);
    return {};
}

QVariantMap ChooserJob::acceptSave(const QString &folder, const QString &name, const QVariantMap &choices, int filter, bool replace) {
    const QList<QByteArray> names = m_request.kind == Kind::SaveFiles ? m_request.names : QList<QByteArray>{QFile::encodeName(name)};
    if (m_request.kind == Kind::SaveFile && !safeFilename(names.first())) {
        return {{"error", tr("Enter a name for the file")}};
    }
    const auto checked = checkDestinations(QFile::encodeName(folder), names);
    if (const QString *why = std::get_if<QString>(&checked)) {
        return {{"error", *why}};
    }
    const Destinations destinations = std::get<Destinations>(checked);
    if (destinations.existingFiles && !replace) {
        QStringList taken;
        for (const QByteArray &path : destinations.paths) {
            if (QFileInfo::exists(QFile::decodeName(path)) || QFileInfo(QFile::decodeName(path)).isSymLink()) {
                taken.append(QFileInfo(QFile::decodeName(path)).fileName());
            }
        }
        return {{"replace", taken}};
    }
    answer(destinations.paths, choices, filter);
    return {};
}

void ChooserJob::cancel() {
    if (m_done) {
        return;
    }
    m_done = true;
    emit finished(1, {});
}

void ChooserJob::answer(const QList<QByteArray> &paths, const QVariantMap &choices, int filter) {
    if (m_done) {
        return;
    }
    m_done = true;
    QStringList uris;
    for (const QByteArray &path : paths) {
        uris.append(localUri(path));
    }
    QVariantMap results{{"uris", uris}};
    QList<ChoiceValue> chosen;
    for (auto it = choices.constBegin(); it != choices.constEnd(); ++it) {
        chosen.append({it.key(), it.value().toString()});
    }
    if (!m_request.choices.isEmpty()) {
        results.insert(QStringLiteral("choices"), QVariant::fromValue(chosen));
    }
    QList<Filter> all = m_request.filters;
    if (m_request.currentFilter && !all.contains(*m_request.currentFilter)) {
        all.append(*m_request.currentFilter);
    }
    if (filter >= 0 && filter < all.size()) {
        results.insert(QStringLiteral("current_filter"), QVariant::fromValue(all.at(filter)));
    }
    emit finished(0, results);
}

ChooserService::ChooserService(QObject *parent) : QObject(parent) { registerTypes(); }

uint ChooserService::OpenFile(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                              const QVariantMap &options, QVariantMap &) {
    return start(handle, appId, parentWindow, openRequest(title, optionsFrom(options)));
}

uint ChooserService::SaveFile(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                              const QVariantMap &options, QVariantMap &) {
    return start(handle, appId, parentWindow, saveFileRequest(title, optionsFrom(options)));
}

uint ChooserService::SaveFiles(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                               const QVariantMap &options, QVariantMap &) {
    return start(handle, appId, parentWindow, saveFilesRequest(title, optionsFrom(options)));
}

uint ChooserService::start(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const Checked &checked) {
    if (const QString *why = std::get_if<QString>(&checked)) {
        sendErrorReply(QStringLiteral("org.freedesktop.portal.Error.InvalidArgument"), *why);
        return 2;
    }
    auto tracked = m_tracker.begin(handle.path());
    if (!tracked) {
        sendErrorReply(QStringLiteral("org.freedesktop.portal.Error.Failed"), tr("Too many file choosers are open"));
        return 2;
    }
    auto *job = new ChooserJob(std::get<Request>(checked), this);
    if (const auto size = Placement::parentSizeHint(appId, parentWindow)) {
        job->setSizeHint(*size);
    }
    QDBusConnection bus = connection();
    bus.registerObject(handle.path(), new RequestObject(job), QDBusConnection::ExportScriptableSlots);
    setDelayedReply(true);
    const QDBusMessage call = message();
    const QString path = handle.path();
    connect(job, &ChooserJob::finished, this, [bus, call, path, job, tracked = *tracked](uint response, const QVariantMap &results) mutable {
        QDBusConnection(bus).unregisterObject(path);
        QDBusMessage reply = call.createReply();
        reply << response << results;
        QDBusConnection(bus).send(reply);
        job->deleteLater();
        // The request ends with its last copy, here.
        tracked = {};
    });
    // Closed before its window showed: answered at once.
    if (tracked->cancelled()) {
        job->cancel();
    } else {
        emit chooserWanted(job);
    }
    return 0;
}

} // namespace Portal

#include "ChooserService.moc"
