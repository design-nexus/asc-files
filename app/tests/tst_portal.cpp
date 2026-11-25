// SPDX-License-Identifier: MIT
//! Legacy's portal tests (`portal/tests.rs`), on the chooser's requests, checks and answers.
#include "portal/Chooser.h"

#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QTest>

using namespace Portal;

namespace {

QByteArray bytes(const QString &path) { return QFile::encodeName(path); }

template <typename T> T value(const std::variant<T, QString> &checked) {
    if (const QString *error = std::get_if<QString>(&checked)) {
        qFatal("unexpected refusal: %s", qPrintable(*error));
    }
    return std::get<T>(checked);
}

Request request(const Checked &checked) { return value(checked); }

bool refused(const Checked &checked) { return std::holds_alternative<QString>(checked); }

Filter filter(const QString &label, QList<Rule> rules) { return {label, rules}; }

Choice boolean(const QString &id, const QString &label, bool initial) { return {id, label, {}, initial ? "true" : "false"}; }

} // namespace

class TestPortal : public QObject {
    Q_OBJECT
private slots:
    void openDefaultsMatchThePortalContract() {
        const Request open = request(openRequest({}, {}));
        QVERIFY(open.modal);
        QCOMPARE(open.title, QStringLiteral("Open Files"));
        QCOMPARE(open.acceptLabel, QStringLiteral("Open"));
        QCOMPARE(open.kind, Kind::Open);
        QVERIFY(!open.directory && !open.multiple);
    }

    void folderHintsOverrideDefaultsAndInvalidHintsPreserveSaveNames() {
        QTemporaryDir current;
        const QList<std::optional<QByteArray>> hints{std::nullopt, QByteArray("relative"), bytes(current.filePath("missing")), bytes(current.path())};
        for (const auto &hint : hints) {
            const QByteArray expected = hint == bytes(current.path()) ? bytes(current.path()) : defaultSaveFolder();
            QCOMPARE(accessibleFolder(hint), expected);
            const auto suggestion = saveFileSuggestion(std::nullopt, hint, QStringLiteral("report.txt"));
            QCOMPARE(suggestion.first, expected);
            QCOMPARE(suggestion.second, std::optional<QByteArray>("report.txt"));
        }
    }

    void currentFileTakesPrecedenceOverFolderAndName() {
        QTemporaryDir current, ignored;
        QFile file(current.filePath("existing.txt"));
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.close();
        const auto suggestion = saveFileSuggestion(bytes(file.fileName()), bytes(ignored.path()), QStringLiteral("ignored.txt"));
        QCOMPARE(suggestion.first, bytes(current.path()));
        QCOMPARE(suggestion.second, std::optional<QByteArray>("existing.txt"));
    }

    void currentFilePreservesANonUtf8Filename() {
        QTemporaryDir current;
        const QByteArray file = bytes(current.path()) + "/n\xff";
        QFile made(QFile::decodeName(file));
        QVERIFY(made.open(QIODevice::WriteOnly));
        made.close();
        QCOMPARE(saveFileSuggestion(file, std::nullopt, std::nullopt), (QPair<QByteArray, std::optional<QByteArray>>{bytes(current.path()), QByteArray("n\xff")}));
    }

    void newCurrentFilePreservesFilenameAndDirectory() {
        QTemporaryDir current;
        for (const QByteArray &name : {QStringLiteral("packing list 格式(2).xls").toUtf8(), QByteArray("n\xff")}) {
            const QByteArray file = bytes(current.path()) + '/' + name;
            QCOMPARE(saveFileSuggestion(file, bytes(current.path()), std::nullopt),
                     (QPair<QByteArray, std::optional<QByteArray>>{bytes(current.path()), name}));
            QVERIFY2(!QFile::exists(QFile::decodeName(file)), "suggesting a name must not create a file");
        }
    }

    void currentFileRejectsDirectoriesAndMissingParents() {
        QTemporaryDir current;
        for (const QByteArray &file : {bytes(current.path()), bytes(current.filePath("missing/new.txt"))}) {
            QCOMPARE(saveFileSuggestion(file, std::nullopt, std::nullopt), (QPair<QByteArray, std::optional<QByteArray>>{defaultSaveFolder(), std::nullopt}));
        }
    }

    void invalidCurrentFileFallsBackWithoutUsingLowerPrioritySuggestions() {
        QTemporaryDir ignored;
        QCOMPARE(saveFileSuggestion(QByteArray("relative/missing.txt"), bytes(ignored.path()), QStringLiteral("ignored.txt")),
                 (QPair<QByteArray, std::optional<QByteArray>>{defaultSaveFolder(), std::nullopt}));
    }

    void localUriPreservesNonUtf8PathBytes() {
        const QString uri = localUri("/tmp/n\xff");
        QVERIFY(uri.startsWith("file://"));
        QVERIFY(uri.toUpper().contains("%FF"));
    }

    void saveFilenamesMustBeSafeBasenames() {
        for (const QByteArray &unsafe : {QByteArray(""), QByteArray("."), QByteArray(".."), QByteArray("a/b")}) {
            QVERIFY2(!safeFilename(unsafe), unsafe.constData());
        }
        QVERIFY(safeFilename("report.txt"));
        QVERIFY(safeFilename("n\xff"));
        QVERIFY(validateSaveNames({}));
        QVERIFY(validateSaveNames({"../bad"}));
    }

    void saveFilesPreserveOrderAndReportCollisionsOnce() {
        QTemporaryDir folder;
        QFile existing(folder.filePath("second"));
        QVERIFY(existing.open(QIODevice::WriteOnly));
        existing.close();
        const Destinations checked = value(checkDestinations(bytes(folder.path()), {"first", "second"}));
        QCOMPARE(checked.paths, (QList<QByteArray>{bytes(folder.filePath("first")), bytes(folder.filePath("second"))}));
        QVERIFY(checked.existingFiles);
    }

    void saveFilesBlockDirectoryCollisions() {
        QTemporaryDir folder;
        QVERIFY(QDir(folder.path()).mkdir("reserved"));
        const auto checked = checkDestinations(bytes(folder.path()), {"reserved"});
        QVERIFY(std::holds_alternative<QString>(checked));
        QVERIFY(std::get<QString>(checked).contains("folder"));
    }

    void filtersAndChoicesKeepInputOrderAndCurrentFilter() {
        const Filter images = filter("Images", {{0, "*.png"}, {1, "image/jpeg"}});
        const Filter text = filter("Text", {{0, "*.txt"}});
        const Choice encoding{"encoding", "Encoding", {{"utf8", "UTF-8"}, {"latin1", "Latin-1"}}, "utf8"};
        Options options;
        options.filters = {images, text};
        options.currentFilter = text;
        options.choices = {boolean("readonly", "Read only", true), encoding};
        const Request open = request(openRequest("Choose", options));
        QCOMPARE(open.filters, (QList<Filter>{images, text}));
        QCOMPARE(open.currentFilter, std::optional<Filter>(text));
        QCOMPARE(open.choices.size(), 2);
        QCOMPARE(open.choices[0].id, QStringLiteral("readonly"));
        QCOMPARE(open.choices[1].id, QStringLiteral("encoding"));
    }

    void openSelectionValidatesKindCardinalityAndLocality() {
        const Chosen file{"/tmp/file", false, true};
        const Chosen folder{"/tmp/folder", true, true};
        QCOMPARE(value(openSelection({file}, "/tmp", false, false)), QList<QByteArray>{"/tmp/file"});
        QVERIFY(std::holds_alternative<QString>(openSelection({file, folder}, "/tmp", false, true)));
        QVERIFY(std::holds_alternative<QString>(openSelection({file, file}, "/tmp", false, false)));
        QCOMPARE(value(openSelection({}, "/tmp", true, false)), QList<QByteArray>{"/tmp"});
        QVERIFY(std::holds_alternative<QString>(openSelection({{"smb://server/share/file", true, false}}, "/tmp", true, false)));
    }

    void cancellationBeforePresentationIsStickyAndCleanupIsRaceSafe() {
        RequestTracker tracker;
        auto first = tracker.begin("same");
        QVERIFY(first);
        QVERIFY(!tracker.begin("same"));
        QVERIFY(tracker.cancel("same"));
        QVERIFY(first->cancelled());
        first.reset();
        auto replacement = tracker.begin("same");
        QVERIFY(replacement);
        QVERIFY(tracker.cancel("same"));
        QVERIFY(replacement->cancelled());
        replacement.reset();
        QVERIFY(!tracker.cancel("same"));
    }

    void backendIdlesOnlyAfterTheLastRequestFinishes() {
        using Clock = RequestTracker::Clock;
        RequestTracker tracker;
        auto first = tracker.begin("first");
        auto second = tracker.begin("second");
        const auto old = Clock::now() - IdleTimeout - std::chrono::seconds(1);
        tracker.setLastActivity(old);
        QVERIFY(!tracker.beginShutdownIfIdle(Clock::now()));
        first.reset();
        QVERIFY(!tracker.beginShutdownIfIdle(Clock::now()));
        second.reset();
        QVERIFY(!tracker.beginShutdownIfIdle(Clock::now()));
        tracker.setLastActivity(old);
        auto third = tracker.begin("third");
        QVERIFY2(third, "a new request resets idle");
        QVERIFY(!tracker.beginShutdownIfIdle(Clock::now()));
        third.reset();
        QVERIFY(!tracker.beginShutdownIfIdle(Clock::now()));
        tracker.setLastActivity(old);
        QVERIFY(tracker.beginShutdownIfIdle(Clock::now()));
        QVERIFY(!tracker.begin("too-late"));
    }

    void untrustedRequestInputsAreBounded() {
        QVERIFY2(validateCommon(QString(MaxStringBytes + 1, u'x'), std::nullopt, {}), "titles must be bounded");
        QList<Choice> choices;
        for (int index = 0; index <= MaxChoices; ++index) {
            choices.append(boolean(QStringLiteral("choice-%1").arg(index), "Choice", false));
        }
        QVERIFY(validateChoices(choices));
        Choice many{"choice", "Choice", {}, "option-0"};
        for (int index = 0; index <= MaxChoiceOptions; ++index) {
            many.options.append({QStringLiteral("option-%1").arg(index), "Option"});
        }
        QVERIFY(validateChoices({many}));
        Filter bulky{"GitHub accepted types", {}};
        for (int index = 0; index <= 1024; ++index) {
            bulky.rules.append({1, QStringLiteral("application/x-attachment-%1").arg(index)});
        }
        QVERIFY(!validateFilters({bulky}, std::nullopt));
        QVERIFY2(validateFilters({filter("Filter", {{0, "*a*a*a*z"}})}, std::nullopt), "backtracking-heavy globs must be rejected");
        QVERIFY2(!validateFilters({filter("Filter", {{0, "*a*.txt"}})}, std::nullopt), "two wildcard groups remain supported");
        QVERIFY2(validateFilters({filter("Filter", {{0, QString(MaxGlobBytes + 1, u'a')}})}, std::nullopt), "glob length must be bounded");
        QVERIFY(validateSaveNames(QList<QByteArray>(MaxSaveFiles + 1, "file")));
        QVERIFY(validateSaveNames({QByteArray(MaxFilenameBytes + 1, 'x')}));
        QVERIFY(validatePath(QByteArray(MaxStringBytes + 1, 'x'), "path"));
    }

    void activeRequestCountIsBounded() {
        RequestTracker tracker;
        QList<RequestTracker::Tracked> requests;
        for (int index = 0; index < MaxActiveRequests; ++index) {
            auto one = tracker.begin(QStringLiteral("request%1").arg(index));
            QVERIFY(one);
            requests.append(*one);
        }
        QVERIFY(!tracker.begin("one-too-many"));
        requests.clear();
        QVERIFY(tracker.begin("available-again"));
    }

    void backendVersionAndSuccessUriSchemeAreFixed() {
        QCOMPARE(FileChooserVersion, 4);
        for (const QByteArray &path : {QByteArray("/tmp/a"), QByteArray("/tmp/a b")}) {
            QVERIFY(localUri(path).startsWith("file://"));
        }
        QCOMPARE(localUri("/tmp/a b"), QStringLiteral("file:///tmp/a%20b"));
        QVERIFY(localUri("relative").isEmpty());
    }

    void longFilterListsAreAccepted() {
        QList<Filter> filters;
        for (int index = 0; index < 256; ++index) {
            filters.append(filter(QStringLiteral("Filter %1").arg(index), {{1, QStringLiteral("application/x-upload-%1").arg(index)}}));
        }
        QVERIFY2(!validateFilters(filters, std::nullopt), "a large well-formed filter list must open the chooser");
        QVERIFY2(!validateFilters(filters, filter("Selected", {{1, "application/x-selected"}})), "a current filter outside a large list must be accepted");
    }

    // New: the three kinds' own checks and defaults.
    void saveRequestsCheckTheirNamesAndFolders() {
        QTemporaryDir folder;
        Options options;
        options.currentFolder = bytes(folder.path());
        options.currentName = QStringLiteral("report.txt");
        const Request save = request(saveFileRequest({}, options));
        QCOMPARE(save.title, QStringLiteral("Save File"));
        QCOMPARE(save.acceptLabel, QStringLiteral("Save"));
        QCOMPARE(save.folder, bytes(folder.path()));
        QCOMPARE(save.currentName, std::optional<QByteArray>("report.txt"));
        options.files = {"a.txt", "b.txt"};
        const Request saveFiles = request(saveFilesRequest("Export", options));
        QCOMPARE(saveFiles.kind, Kind::SaveFiles);
        QCOMPARE(saveFiles.names, (QList<QByteArray>{"a.txt", "b.txt"}));
        options.files = {"../escape"};
        QVERIFY(refused(saveFilesRequest({}, options)));
    }
};

QTEST_GUILESS_MAIN(TestPortal)
#include "tst_portal.moc"
