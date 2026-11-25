// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FolderModel.h"
#include "models/JobModel.h"
#include "models/Transfers.h"
#include "ops/Operations.h"

#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QUrl>

namespace {

void touch(const QString &path, const QByteArray &contents = "x") {
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
}

} // namespace

class TestTransfers : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void aDragCarriesTheSelection() {
        QTemporaryDir dir;
        touch(dir.filePath("a.txt"));
        touch(dir.filePath("b.txt"));
        touch(dir.filePath("c.txt"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel folder(&backend);
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        QSignalSpy opened(&folder, &FolderModel::opened);
        folder.openPath(dir.path());
        QVERIFY(opened.wait(10000));
        folder.data(folder.index(0), FolderModel::NameRole);
        QTRY_VERIFY(folder.isLoaded(0));
        folder.select(0);
        folder.toggle(2);
        const QStringList uris = transfers.dragUris(&folder, 2).split("\r\n");
        QCOMPARE(uris, (QStringList{QUrl::fromLocalFile(dir.filePath("a.txt")).toString(),
                                    QUrl::fromLocalFile(dir.filePath("c.txt")).toString()}));
        // Dragging a row outside the selection takes that row alone.
        QCOMPARE(transfers.dragUris(&folder, 1), QUrl::fromLocalFile(dir.filePath("b.txt")).toString());
        QCOMPARE(folder.selectionCount(), 1);
    }

    void aDropOnTheSameDriveMovesAndAsksAboutAClash() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("into"));
        touch(dir.filePath("a.txt"), "moved");
        touch(dir.filePath("b.txt"), "no clash");
        touch(dir.filePath("into/a.txt"), "already there");
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        const QList<QUrl> urls{QUrl::fromLocalFile(dir.filePath("a.txt")), QUrl::fromLocalFile(dir.filePath("b.txt"))};
        QVERIFY(transfers.movesInto(urls, dir.filePath("into")));
        QSignalSpy asked(&ops, &Operations::conflictsChanged);
        QVERIFY(transfers.drop(urls, dir.filePath("into")));
        QVERIFY(asked.wait(10000));
        QCOMPARE(ops.conflicts().size(), 1);
        QCOMPARE(ops.conflicts().first().toMap().value("name").toString(), QStringLiteral("a.txt"));
        // Kept both: the dropped one numbered; the one without a clash simply moved.
        QSignalSpy finished(&jobs, &JobModel::finished);
        ops.resolve({Operations::KeepBoth});
        QVERIFY(finished.wait(20000));
        QVERIFY(!QFile::exists(dir.filePath("a.txt")));
        QVERIFY(!QFile::exists(dir.filePath("b.txt")));
        QVERIFY(QFile::exists(dir.filePath("into/b.txt")));
        QFile kept(dir.filePath("into/a.txt"));
        QVERIFY(kept.open(QIODevice::ReadOnly));
        QCOMPARE(kept.readAll(), QByteArray("already there"));
        QVERIFY(QFile::exists(dir.filePath("into/a (1).txt")));
    }

    void aDropWithoutClashesStartsAtOnce() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("into"));
        touch(dir.filePath("a.txt"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        QSignalSpy finished(&jobs, &JobModel::finished);
        QVERIFY(transfers.drop({QUrl::fromLocalFile(dir.filePath("a.txt"))}, dir.filePath("into")));
        QVERIFY(finished.wait(20000));
        QVERIFY(QFile::exists(dir.filePath("into/a.txt")));
        QCOMPARE(ops.conflicts().size(), 0);
    }

    void dropsThatWouldDoNothingAreRefused() {
        QTemporaryDir dir;
        QDir().mkpath(dir.filePath("folder/inner"));
        touch(dir.filePath("here.txt"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        Operations ops(&backend, &jobs);
        Transfers transfers(&ops);
        // Into the folder it is already in, into itself, into a folder below it.
        QVERIFY(!transfers.drop({QUrl::fromLocalFile(dir.filePath("here.txt"))}, dir.path()));
        QVERIFY(!transfers.drop({QUrl::fromLocalFile(dir.filePath("folder"))}, dir.filePath("folder")));
        QVERIFY(!transfers.drop({QUrl::fromLocalFile(dir.filePath("folder"))}, dir.filePath("folder/inner")));
        QVERIFY(!transfers.drop({QUrl(QStringLiteral("https://example.com/x"))}, dir.path()));
        QCOMPARE(jobs.rowCount(), 0);
    }
};

QTEST_MAIN(TestTransfers)
#include "tst_transfers.moc"
