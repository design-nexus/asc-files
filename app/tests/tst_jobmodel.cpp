// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "backend/Wire.h"
#include "models/JobModel.h"

#include <QAbstractItemModelTester>
#include <QFile>
#include <QJsonArray>
#include <QSignalSpy>
#include <QTemporaryDir>

class TestJobModel : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void aCopyIsFollowedToItsEnd() {
        QTemporaryDir dir;
        QDir().mkdir(dir.filePath("from"));
        QDir().mkdir(dir.filePath("to"));
        QFile file(dir.filePath("from/a.bin"));
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.write(QByteArray(300'000, 'a'));
        file.close();

        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        QAbstractItemModelTester tester(&jobs, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy finished(&jobs, &JobModel::finished);
        qint64 started = -2;
        jobs.start({{"into", Wire::encode(QFile::encodeName(dir.filePath("to")))},
                    {"items", QJsonArray{QJsonObject{{"source", dir.filePath("from/a.bin")}, {"action", "copy"}}}}},
                   QStringLiteral("Copy a.bin"), [&](qint64 job, const BackendError &error) {
                       QVERIFY2(error.isNull(), qPrintable(error.message));
                       started = job;
                   });
        QVERIFY(finished.wait(20000));
        QCOMPARE(finished.first().at(0).toLongLong(), started);
        QCOMPARE(jobs.rowCount(), 1);
        const QModelIndex row = jobs.index(0);
        QCOMPARE(jobs.data(row, JobModel::LabelRole).toString(), QStringLiteral("Copy a.bin"));
        QCOMPARE(jobs.data(row, JobModel::StateRole).toInt(), int(JobModel::Finished));
        QCOMPARE(jobs.data(row, JobModel::DoneRole).toInt(), 1);
        QCOMPARE(jobs.data(row, JobModel::TotalBytesRole).toLongLong(), 300'000);
        QCOMPARE(jobs.data(row, JobModel::BytesRole).toLongLong(), 300'000);
        QCOMPARE(jobs.data(row, JobModel::ProgressRole).toDouble(), 1.0);
        QVERIFY(jobs.data(row, JobModel::UndoRole).toLongLong() > 0);
        QCOMPARE(jobs.running(), 0);
        QVERIFY(QFile::exists(dir.filePath("to/a.bin")));
        jobs.clearFinished();
        QCOMPARE(jobs.rowCount(), 0);
    }

    void aFailedItemGivesItsMessage() {
        QTemporaryDir dir;
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        QSignalSpy finished(&jobs, &JobModel::finished);
        jobs.start({{"into", dir.path()},
                    {"items", QJsonArray{QJsonObject{{"source", dir.filePath("missing")}, {"action", "copy"}}}}},
                   QStringLiteral("Copy missing"));
        QVERIFY(finished.wait(20000));
        QCOMPARE(jobs.data(jobs.index(0), JobModel::FailedRole).toInt(), 1);
        QVERIFY(!jobs.data(jobs.index(0), JobModel::ErrorRole).toString().isEmpty());
    }

    void aRefusedBatchStartsNoJob() {
        BackendClient backend(TestSupport::backend());
        backend.start();
        JobModel jobs(&backend);
        BackendError seen;
        bool answered = false;
        jobs.start({{"into", "relative"}, {"items", QJsonArray{}}}, QString(), [&](qint64 job, const BackendError &error) {
            QCOMPARE(job, -1);
            seen = error;
            answered = true;
        });
        QTRY_VERIFY_WITH_TIMEOUT(answered, 10000);
        QCOMPARE(seen.code, -32602);
        QCOMPARE(jobs.rowCount(), 0);
    }
};

QTEST_GUILESS_MAIN(TestJobModel)
#include "tst_jobmodel.moc"
