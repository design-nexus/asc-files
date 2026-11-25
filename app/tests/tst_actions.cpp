// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/ActionsModel.h"

#include <QAbstractItemModelTester>
#include <QDir>
#include <QFile>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>

namespace {

void write(const QString &path, const QByteArray &contents) {
    QDir().mkpath(QFileInfo(path).path());
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
}

QStringList ids(const QVariantList &actions) {
    QStringList ids;
    for (const QVariant &action : actions) {
        ids.append(action.toMap().value("id").toString());
    }
    return ids;
}

} // namespace

class TestActions : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void actionsAreOfferedRunAndEdited() {
        const QString folder = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + "/asc-files/actions/count";
        QVERIFY(folder.contains("asc-files-test-home"));
        QDir(folder).removeRecursively();
        BackendClient backend(TestSupport::backend());
        ActionsModel actions(&backend);
        QAbstractItemModelTester tester(&actions, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy listed(&actions, &ActionsModel::listChanged);
        backend.start();
        QVERIFY(listed.wait(10000));

        QSignalSpy saved(&actions, &ActionsModel::saved);
        QSignalSpy failed(&actions, &ActionsModel::failed);
        actions.write({{"schema_version", 1}, {"id", "count"}, {"name", "Count lines"}, {"enabled", true},
                       {"when", QVariantMap{{"kinds", QStringList{"file"}}, {"extensions", QStringList{"txt"}}}},
                       {"run", QVariantMap{{"runtime", "bash"}, {"entrypoint", "run.sh"}, {"mode", "per-item"}}}},
                      {{"fileName", "run.sh"},
                       {"contents", "#!/usr/bin/env bash\nwhile IFS= read -r -d '' path; do wc -l < \"$path\"; done < \"$NYX_FILES_ACTION_PATHS\"\n"}},
                      true);
        QVERIFY2(saved.wait(10000), qPrintable(failed.isEmpty() ? QString() : failed.first().first().toString()));
        QTRY_VERIFY(ids(actions.actions()).contains("count"));

        // Offered for a text file, not for a picture.
        QTemporaryDir dir;
        write(dir.filePath("notes.txt"), "one\ntwo\nthree\n");
        write(dir.filePath("photo.png"), "\x89PNG");
        actions.offer({dir.filePath("notes.txt")});
        QTRY_COMPARE(ids(actions.offered()), QStringList{"count"});
        actions.offer({dir.filePath("photo.png")});
        QTest::qWait(300);
        QVERIFY(actions.offered().isEmpty());

        const QVariantMap count = actions.actions().first().toMap();
        if (!count.value("available").toBool()) {
            QSKIP("bash is not installed");
        }
        QSignalSpy started(&actions, &ActionsModel::started);
        actions.run("count", {dir.filePath("notes.txt")}, dir.path());
        QVERIFY(started.wait(10000));
        // Its runs come as the backend says how they go.
        QTRY_COMPARE(actions.rowCount(), 1);
        QTRY_COMPARE_WITH_TIMEOUT(actions.data(actions.index(0), ActionsModel::StatusRole).toString(), QStringLiteral("succeeded"), 10000);
        QVERIFY(actions.data(actions.index(0), ActionsModel::LogRole).toString().contains('3'));
        QCOMPARE(actions.running(), 0);
        actions.clearFinished();
        QCOMPARE(actions.rowCount(), 0);

        // Edited from Settings.
        actions.setEnabled("count", false);
        QTRY_VERIFY(!actions.actions().first().toMap().value("enabled").toBool());
        actions.run("count", {dir.filePath("notes.txt")}, dir.path());
        QVERIFY(failed.wait(10000));
        actions.duplicate("count");
        QTRY_COMPARE(ids(actions.actions()), (QStringList{"count", "count-copy"}));
        QSignalSpy read(&actions, &ActionsModel::readDone);
        actions.read("count-copy");
        QVERIFY(read.wait(10000));
        QCOMPARE(read.first().first().toMap().value("definition").toMap().value("name").toString(), QStringLiteral("Count lines (copy)"));
        actions.remove("count-copy");
        actions.remove("count");
        QTRY_VERIFY(actions.actions().isEmpty());

        QSignalSpy examples(&actions, &ActionsModel::examplesRead);
        actions.readExamples();
        QVERIFY(examples.wait(10000));
        QVERIFY(examples.first().first().toList().size() >= 10);
    }
};

QTEST_GUILESS_MAIN(TestActions)
#include "tst_actions.moc"
