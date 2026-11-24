// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <csignal>

class TestBackendClient : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void startsASessionAndAnswersCalls() {
        BackendClient client(TestSupport::backend());
        QSignalSpy ready(&client, &BackendClient::ready);
        // Asked before the process runs: sent once it does.
        bool answered = false;
        client.call(QStringLiteral("undo.history"), {}, [&](const QJsonValue &result, const BackendError &error) {
            QVERIFY2(error.isNull(), qPrintable(error.message));
            QVERIFY(result.toObject().contains("undo"));
            answered = true;
        });
        client.start();
        QVERIFY(ready.wait(10000));
        QVERIFY(client.isReady());
        QVERIFY(client.capabilities().contains(QStringLiteral("ops.rename")));
        QTRY_VERIFY(answered);
    }

    void refusalsCarryTheirKind() {
        BackendClient client(TestSupport::backend());
        client.start();
        BackendError seen;
        bool answered = false;
        client.call(QStringLiteral("list.open"), {{"path", "/no/such/folder/here"}},
                    [&](const QJsonValue &, const BackendError &error) {
                        seen = error;
                        answered = true;
                    });
        QTRY_VERIFY_WITH_TIMEOUT(answered, 10000);
        QCOMPARE(seen.code, 1);
        QCOMPARE(seen.kind, QStringLiteral("notFound"));
        QVERIFY(!seen.message.isEmpty());

        answered = false;
        client.call(QStringLiteral("list.open"), {{"path", "relative"}}, [&](const QJsonValue &, const BackendError &error) {
            seen = error;
            answered = true;
        });
        QTRY_VERIFY(answered);
        QCOMPARE(seen.code, -32602);
        QVERIFY(seen.kind.isEmpty());
    }

    void aBackendThatDiesIsStartedAgain() {
        BackendClient client(TestSupport::backend());
        QSignalSpy ready(&client, &BackendClient::ready);
        QSignalSpy restarted(&client, &BackendClient::restarted);
        client.start();
        QVERIFY(ready.wait(10000));

        // A call the dead process never answers fails with Stopped.
        BackendError seen;
        bool answered = false;
        QTemporaryDir dir;
        client.call(QStringLiteral("jobs.clashes"), {{"into", dir.path()}, {"sources", QJsonArray{}}},
                    [&](const QJsonValue &, const BackendError &error) {
                        seen = error;
                        answered = true;
                    });
        ::kill(pid_t(client.processId()), SIGKILL);
        QVERIFY(restarted.wait(10000));
        QVERIFY(client.isReady());
        if (answered) {
            // The reply may have beaten the kill; otherwise the call failed with Stopped.
            QVERIFY(seen.isNull() || seen.code == BackendError::Stopped);
        }

        bool after = false;
        client.call(QStringLiteral("undo.history"), {}, [&](const QJsonValue &, const BackendError &error) {
            after = error.isNull();
        });
        QTRY_VERIFY(after);
    }

    void aBackendThatKeepsDyingIsGivenUp() {
        BackendClient client(TestSupport::backend());
        QSignalSpy ready(&client, &BackendClient::ready);
        QSignalSpy failed(&client, &BackendClient::failed);
        client.start();
        for (int death = 0; death < 3; ++death) {
            QVERIFY(ready.wait(10000) || client.isReady());
            ready.clear();
            ::kill(pid_t(client.processId()), SIGKILL);
            if (death < 2) {
                QTRY_VERIFY_WITH_TIMEOUT(!client.isReady(), 5000);
            }
        }
        QVERIFY(failed.wait(10000));
        bool answered = false;
        client.call(QStringLiteral("undo.history"), {}, [&](const QJsonValue &, const BackendError &error) {
            answered = error.code == BackendError::Stopped;
        });
        QTRY_VERIFY(answered);
    }

    void aReplyForAnOwnerThatIsGoneIsDropped() {
        BackendClient client(TestSupport::backend());
        client.start();
        auto *owner = new QObject;
        bool answered = false;
        client.call(QStringLiteral("undo.history"), {}, owner, [&](const QJsonValue &, const BackendError &) { answered = true; });
        delete owner;
        bool after = false;
        client.call(QStringLiteral("undo.history"), {}, [&](const QJsonValue &, const BackendError &) { after = true; });
        QTRY_VERIFY_WITH_TIMEOUT(after, 10000);
        QVERIFY(!answered);
    }

    void aMissingProgramFails() {
        BackendClient client(QStringLiteral("/no/such/asc-files-backend"));
        QSignalSpy failed(&client, &BackendClient::failed);
        client.start();
        QVERIFY(failed.wait(10000));
    }
};

QTEST_GUILESS_MAIN(TestBackendClient)
#include "tst_backendclient.moc"
