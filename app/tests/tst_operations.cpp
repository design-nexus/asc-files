// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/TrashModel.h"
#include "ops/Operations.h"

#include <QClipboard>
#include <QFile>
#include <QGuiApplication>
#include <QMimeData>
#include <QUrl>
#include <QSignalSpy>
#include <QTemporaryDir>

namespace {

void write(const QString &path, const QByteArray &contents) {
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
}

QByteArray read(const QString &path) {
    QFile file(path);
    return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
}

/// A model of `path` with every row read.
bool openAll(FolderModel &model, const QString &path) {
    QSignalSpy opened(&model, &FolderModel::opened);
    model.openPath(path);
    if (!opened.wait(10000)) {
        return false;
    }
    for (int row = 0; row < model.rowCount(); ++row) {
        model.data(model.index(row), FolderModel::NameRole);
    }
    return QTest::qWaitFor([&] { return model.rowCount() == 0 || model.isLoaded(model.rowCount() - 1); }, 10000);
}

int rowOf(FolderModel &model, const QString &name) {
    for (int row = 0; row < model.rowCount(); ++row) {
        if (model.data(model.index(row), FolderModel::NameRole).toString() == name) {
            return row;
        }
    }
    return -1;
}

} // namespace

class TestOperations : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void aPasteAsksAboutClashesOnly() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("from"));
        QDir().mkdir(dir.filePath("to"));
        write(dir.filePath("from/new.txt"), "new");
        write(dir.filePath("from/replace.txt"), "incoming");
        write(dir.filePath("from/both.txt"), "incoming");
        write(dir.filePath("from/skip.txt"), "incoming");
        for (const char *name : {"replace.txt", "both.txt", "skip.txt"}) {
            write(dir.filePath(QStringLiteral("to/") + QLatin1String(name)), "existing");
        }
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel from(&backend), to(&backend);
        QVERIFY(openAll(from, dir.filePath("from")));
        QVERIFY(openAll(to, dir.filePath("to")));
        from.selectAll();
        ops.copy(&from, false);
        QVERIFY(ops.canPaste());

        QSignalSpy asked(&ops, &Operations::conflictsChanged);
        ops.paste(&to);
        QVERIFY(asked.wait(10000));
        const QVariantList conflicts = ops.conflicts();
        QCOMPARE(conflicts.size(), 3);
        QStringList names;
        QList<int> choices;
        for (const QVariant &conflict : conflicts) {
            const QString name = conflict.toMap().value("name").toString();
            names.append(name);
            choices.append(name == "replace.txt" ? Operations::Replace : name == "both.txt" ? Operations::KeepBoth : Operations::Skip);
            QCOMPARE(conflict.toMap().value("existing").toMap().value("size").toInt(), 8);
        }
        QCOMPARE(names, (QStringList{"both.txt", "replace.txt", "skip.txt"}));
        QSignalSpy finished(&jobs, &JobModel::finished);
        ops.resolve(choices);
        QVERIFY(ops.conflicts().isEmpty());
        QVERIFY(finished.wait(20000));
        QCOMPARE(read(dir.filePath("to/new.txt")), QByteArray("new"));
        QCOMPARE(read(dir.filePath("to/replace.txt")), QByteArray("incoming"));
        QCOMPARE(read(dir.filePath("to/both.txt")), QByteArray("existing"));
        QCOMPARE(read(dir.filePath("to/both (1).txt")), QByteArray("incoming"));
        QCOMPARE(read(dir.filePath("to/skip.txt")), QByteArray("existing"));
        QVERIFY(QFile::exists(dir.filePath("from/replace.txt")));

        // Undo takes the batch back: the replaced original returns.
        QSignalSpy history(&ops, &Operations::historyChanged);
        QTRY_VERIFY(!ops.undoLabel().isEmpty());
        ops.undo();
        QTRY_COMPARE_WITH_TIMEOUT(read(dir.filePath("to/replace.txt")), QByteArray("existing"), 10000);
        QTRY_VERIFY(!QFile::exists(dir.filePath("to/new.txt")));
    }

    // Merging a folder asks next about what clashes inside it, each answered on its own.
    void aMergedFolderAsksAboutItsOwnClashes() {
        QTemporaryDir dir;
        for (const char *path : {"from/photos/2024", "to/photos/2024"}) {
            QDir().mkpath(dir.filePath(path));
        }
        write(dir.filePath("from/photos/new.jpg"), "new");
        write(dir.filePath("from/photos/2024/keep.jpg"), "incoming");
        write(dir.filePath("from/photos/2024/over.jpg"), "incoming");
        write(dir.filePath("to/photos/2024/keep.jpg"), "existing");
        write(dir.filePath("to/photos/2024/over.jpg"), "existing");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel from(&backend), to(&backend);
        QVERIFY(openAll(from, dir.filePath("from")));
        QVERIFY(openAll(to, dir.filePath("to")));
        from.selectAll();
        ops.copy(&from, false);

        QSignalSpy asked(&ops, &Operations::conflictsChanged);
        ops.paste(&to);
        QVERIFY(asked.wait(10000));
        QCOMPARE(ops.conflicts().size(), 1);
        QVERIFY(ops.conflicts().first().toMap().value("canMerge").toBool());
        asked.clear();
        ops.resolve({Operations::Merge});
        // The folder's own clashes, by where they are in it; none can merge.
        QTRY_COMPARE_WITH_TIMEOUT(ops.conflicts().size(), 2, 10000);
        QStringList where;
        for (const QVariant &conflict : ops.conflicts()) {
            where.append(conflict.toMap().value("folder").toString() + u'/' + conflict.toMap().value("name").toString());
            QVERIFY(!conflict.toMap().value("canMerge").toBool());
        }
        QCOMPARE(where, (QStringList{"photos/2024/keep.jpg", "photos/2024/over.jpg"}));
        QSignalSpy finished(&jobs, &JobModel::finished);
        ops.resolve({Operations::Skip, Operations::Replace});
        QVERIFY(finished.wait(20000));
        QCOMPARE(read(dir.filePath("to/photos/new.jpg")), QByteArray("new"));
        QCOMPARE(read(dir.filePath("to/photos/2024/keep.jpg")), QByteArray("existing"));
        QCOMPARE(read(dir.filePath("to/photos/2024/over.jpg")), QByteArray("incoming"));
        QVERIFY(!QFile::exists(dir.filePath("to/photos/2024/keep (1).jpg")));
    }

    void twoItemsOnOneNameKeepTheOneThatNeedsNoFix() {
        // Both land on "same.txt" (the second's own name; in a real FAT paste, a:b and a_b).
        QTemporaryDir dir;
        QDir().mkpath(dir.filePath("one"));
        QDir().mkpath(dir.filePath("two"));
        QDir().mkdir(dir.filePath("to"));
        write(dir.filePath("one/same.txt"), "first");
        write(dir.filePath("two/same.txt"), "second");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel to(&backend);
        QVERIFY(openAll(to, dir.filePath("to")));
        auto *data = new QMimeData;
        data->setUrls({QUrl::fromLocalFile(dir.filePath("one/same.txt")), QUrl::fromLocalFile(dir.filePath("two/same.txt"))});
        QGuiApplication::clipboard()->setMimeData(data);
        QSignalSpy finished(&jobs, &JobModel::finished);
        ops.paste(&to);
        QVERIFY(finished.wait(20000));
        QVERIFY(ops.conflicts().isEmpty());
        QCOMPARE(read(dir.filePath("to/same.txt")), QByteArray("first"));
        QCOMPARE(read(dir.filePath("to/same (1).txt")), QByteArray("second"));
    }

    void originalsLeftBesideAFolderArePutBack() {
        QTemporaryDir dir;
        const QString container = dir.filePath(".asc-files-originals-0123abcd-4567-89ab-cdef-0123456789ab");
        QDir().mkdir(container);
        write(container + "/song.flac", "old");
        write(dir.filePath("song.flac"), "new");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel here(&backend);
        QSignalSpy leftovers(&here, &FolderModel::leftoversChanged);
        QVERIFY(openAll(here, dir.path()));
        QTRY_COMPARE(here.leftovers(), QStringList{QFileInfo(container).fileName()});
        QSignalSpy reviewed(&ops, &Operations::leftoversReviewed);
        ops.reviewLeftovers(&here);
        QVERIFY(reviewed.wait(10000));
        const QVariantMap first = reviewed.first().first().toList().first().toMap();
        QCOMPARE(first.value("kind").toString(), QStringLiteral("items"));
        QCOMPARE(first.value("names").toStringList(), QStringList{"song.flac"});
        QSignalSpy told(&ops, &Operations::notice);
        ops.recover(&here, QFileInfo(container).fileName());
        QVERIFY(told.wait(10000));
        // The name was taken: the original came back numbered, nothing replaced.
        QCOMPARE(read(dir.filePath("song.flac")), QByteArray("new"));
        QCOMPARE(read(dir.filePath("song (1).flac")), QByteArray("old"));
        QTRY_VERIFY_WITH_TIMEOUT(here.leftovers().isEmpty(), 10000);
    }

    void undoAsksBeforePuttingBackOverANewerItem() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("to"));
        write(dir.filePath("report.txt"), "moved");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel here(&backend), to(&backend);
        QVERIFY(openAll(here, dir.path()));
        QVERIFY(openAll(to, dir.filePath("to")));
        QSignalSpy finished(&jobs, &JobModel::finished);
        here.select(rowOf(here, "report.txt"));
        ops.copy(&here, true);
        ops.paste(&to);
        QVERIFY(finished.wait(20000));
        write(dir.filePath("report.txt"), "newer");
        QTRY_VERIFY(!ops.undoLabel().isEmpty());
        QSignalSpy blocked(&ops, &Operations::undoBlocked);
        ops.undo();
        QVERIFY(blocked.wait(10000));
        QCOMPARE(blocked.first().first().toStringList(), QStringList{"report.txt"});
        QCOMPARE(read(dir.filePath("report.txt")), QByteArray("newer"));
        QSignalSpy told(&ops, &Operations::notice);
        ops.undo(true);
        QVERIFY(told.wait(10000));
        QCOMPARE(read(dir.filePath("report.txt")), QByteArray("moved"));
    }

    void aCutMovesOnceAndACopyIntoItsOwnFolderDuplicates() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("to"));
        write(dir.filePath("item.txt"), "i");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel here(&backend), to(&backend);
        QVERIFY(openAll(here, dir.path()));
        QVERIFY(openAll(to, dir.filePath("to")));
        QSignalSpy finished(&jobs, &JobModel::finished);

        here.select(rowOf(here, "item.txt"));
        ops.copy(&here, false);
        ops.paste(&here);
        QVERIFY(finished.wait(20000));
        QVERIFY(QFile::exists(dir.filePath("item (1).txt")));

        QTRY_VERIFY(rowOf(here, "item (1).txt") >= 0 || (openAll(here, dir.path()) && rowOf(here, "item (1).txt") >= 0));
        here.select(rowOf(here, "item.txt"));
        ops.copy(&here, true);
        ops.paste(&to);
        QVERIFY(finished.wait(20000));
        QVERIFY(QFile::exists(dir.filePath("to/item.txt")));
        QVERIFY(!QFile::exists(dir.filePath("item.txt")));
        QVERIFY(!ops.canPaste());
    }

    void trashNewFolderAndUndo() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("ops-XXXXXX")));
        write(dir.filePath("doomed.txt"), "d");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        FolderModel here(&backend);
        QVERIFY(openAll(here, dir.path()));
        QSignalSpy finished(&jobs, &JobModel::finished);
        here.select(rowOf(here, "doomed.txt"));
        ops.trash(&here);
        QVERIFY(finished.wait(20000));
        QVERIFY(!QFile::exists(dir.filePath("doomed.txt")));
        QTRY_VERIFY(!ops.undoLabel().isEmpty());
        ops.undo();
        QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(dir.filePath("doomed.txt")), 10000);

        // Trashed again, then put back from the Trash.
        here.select(rowOf(here, "doomed.txt") >= 0 ? rowOf(here, "doomed.txt") : (openAll(here, dir.path()), rowOf(here, "doomed.txt")));
        ops.trash(&here);
        QVERIFY(finished.wait(20000));
        QVERIFY(!QFile::exists(dir.filePath("doomed.txt")));
        TrashModel trash(&backend);
        QTRY_VERIFY_WITH_TIMEOUT(trash.rowCount() > 0, 10000);
        int entry = -1;
        for (int row = 0; row < trash.rowCount(); ++row) {
            if (trash.originalAt(row) == dir.filePath("doomed.txt")) {
                entry = row;
            }
        }
        QVERIFY(entry >= 0);
        ops.restore(trash.pathAt(entry));
        QVERIFY(finished.wait(20000));
        QVERIFY(QFile::exists(dir.filePath("doomed.txt")));

        QSignalSpy created(&ops, &Operations::created);
        ops.create(&here, true);
        QVERIFY(created.wait(10000));
        QCOMPARE(created.first().at(1).toString(), QStringLiteral("New Folder"));
        ops.create(&here, true);
        QVERIFY(created.wait(10000));
        QCOMPARE(created.last().at(1).toString(), QStringLiteral("New Folder (1)"));
        QVERIFY(QFileInfo(dir.filePath("New Folder (1)")).isDir());
    }

    void itemsNamedByPathAreCopiedAndTrashed() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("ops-XXXXXX")));
        QDir().mkdir(dir.filePath("from"));
        QDir().mkdir(dir.filePath("into"));
        write(dir.filePath("from/found.txt"), "f");
        write(dir.filePath("from/gone.txt"), "g");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        // As from search results: copied by path, pasted elsewhere.
        ops.copyPaths({dir.filePath("from/found.txt")}, false);
        QVERIFY(ops.canPaste());
        FolderModel into(&backend);
        QVERIFY(openAll(into, dir.filePath("into")));
        QSignalSpy finished(&jobs, &JobModel::finished);
        ops.paste(&into);
        QVERIFY(finished.wait(20000));
        QVERIFY(QFile::exists(dir.filePath("into/found.txt")));
        QVERIFY(QFile::exists(dir.filePath("from/found.txt")));
        // The location as text.
        ops.copyPathText({dir.filePath("from/gone.txt")});
        QCOMPARE(QGuiApplication::clipboard()->text(), dir.filePath("from/gone.txt"));
        ops.trashPaths({dir.filePath("from/gone.txt")});
        QVERIFY(finished.wait(20000));
        QVERIFY(!QFile::exists(dir.filePath("from/gone.txt")));
    }

    void anArchiveIsMadeAndOpenedWithItsPassword() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("ops-XXXXXX")));
        QDir().mkdir(dir.filePath("docs"));
        write(dir.filePath("docs/a.txt"), "a");
        write(dir.filePath("b.txt"), "b");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        QCOMPARE(Operations::archiveFormat("x.ZIP"), QStringLiteral("zip"));
        QCOMPARE(Operations::archiveFormat("x.tgz"), QStringLiteral("tar.gz"));
        QCOMPARE(Operations::archiveFormat("x.docx"), QString());
        QSignalSpy archived(&ops, &Operations::archived);
        QSignalSpy wanted(&ops, &Operations::passwordWanted);

        // Two items: one archive of both, beside them, selected once made.
        ops.compress({dir.filePath("docs"), dir.filePath("b.txt")}, "pack", "zip", "secret");
        QTRY_COMPARE_WITH_TIMEOUT(archived.size(), 1, 20000);
        QCOMPARE(archived.takeFirst().at(0).toString(), dir.filePath("pack.zip"));
        // A second one of the same name is numbered.
        ops.compress({dir.filePath("b.txt")}, "pack", "zip");
        QTRY_COMPARE_WITH_TIMEOUT(archived.size(), 1, 20000);
        QVERIFY(archived.takeFirst().at(0).toString() != dir.filePath("pack.zip"));

        // Opening an archive extracts it; a locked one asks for its password, and again if wrong.
        ops.open(dir.filePath("pack.zip"));
        QTRY_COMPARE_WITH_TIMEOUT(wanted.size(), 1, 20000);
        QCOMPARE(wanted.first().at(0).toString(), dir.filePath("pack.zip"));
        QCOMPARE(wanted.first().at(1).toString(), dir.path());
        QCOMPARE(wanted.takeFirst().at(2).toBool(), false);
        QDir().mkdir(dir.filePath("out"));
        ops.extract(dir.filePath("pack.zip"), dir.filePath("out"), "wrong");
        QTRY_COMPARE_WITH_TIMEOUT(wanted.size(), 1, 20000);
        QCOMPARE(wanted.takeFirst().at(2).toBool(), true);
        ops.extract(dir.filePath("pack.zip"), dir.filePath("out"), "secret");
        QTRY_COMPARE_WITH_TIMEOUT(archived.size(), 1, 20000);
        QCOMPARE(archived.takeFirst().at(0).toString(), dir.filePath("out/pack"));
        QCOMPARE(read(dir.filePath("out/pack/docs/a.txt")), QByteArray("a"));
        QCOMPARE(read(dir.filePath("out/pack/b.txt")), QByteArray("b"));
    }
};

QTEST_MAIN(TestOperations)
#include "tst_operations.moc"
