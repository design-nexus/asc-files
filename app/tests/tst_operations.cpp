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
};

QTEST_MAIN(TestOperations)
#include "tst_operations.moc"
