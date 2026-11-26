// SPDX-License-Identifier: MIT
//! Legacy's `portal/window_geometry` tests, against a fake compositor socket.
#include "portal/Placement.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QFile>
#include <QTemporaryDir>
#include <QTest>
#include <sys/socket.h>
#include <sys/un.h>
#include <thread>
#include <unistd.h>

using namespace Portal::Placement;
using namespace std::chrono_literals;

namespace {

QJsonObject window(const QString &klass, int width, int height) {
    return {{"class", klass}, {"initialClass", klass}, {"mapped", true}, {"hidden", false}, {"size", QJsonArray{width, height}}};
}

std::optional<QSize> size(const QList<QJsonObject> &windows, const QString &appId) {
    QJsonArray array;
    for (const QJsonObject &one : windows) {
        array.append(one);
    }
    return applicationWindowSize(QJsonDocument(array).toJson(), appId);
}

/// A compositor socket answering each connection in turn: reads its command, waits `delay`,
/// replies. What it received comes back once it has served every reply.
class FakeCompositor {
public:
    FakeCompositor(const QByteArray &path, QList<QByteArray> replies, std::chrono::milliseconds delay) {
        m_fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        sockaddr_un address{};
        address.sun_family = AF_UNIX;
        memcpy(address.sun_path, path.constData(), path.size());
        ::bind(m_fd, reinterpret_cast<sockaddr *>(&address), sizeof(address));
        ::listen(m_fd, 8);
        m_thread = std::thread([this, replies, delay] {
            for (const QByteArray &reply : replies) {
                const int client = ::accept(m_fd, nullptr, nullptr);
                if (client < 0) {
                    return;
                }
                char buffer[4096];
                const ssize_t got = ::recv(client, buffer, sizeof(buffer), 0);
                m_received.append(QByteArray(buffer, got > 0 ? got : 0));
                std::this_thread::sleep_for(delay);
                ::send(client, reply.constData(), size_t(reply.size()), MSG_NOSIGNAL);
                ::close(client);
            }
        });
    }
    ~FakeCompositor() {
        ::shutdown(m_fd, SHUT_RDWR);
        ::close(m_fd);
        if (m_thread.joinable()) {
            m_thread.join();
        }
    }
    QList<QByteArray> received() {
        if (m_thread.joinable()) {
            m_thread.join();
        }
        return m_received;
    }

private:
    int m_fd = -1;
    std::thread m_thread;
    QList<QByteArray> m_received;
};

} // namespace

class TestPlacement : public QObject {
    Q_OBJECT
private slots:
    void uniqueRequestingApplicationUsesLogicalWindowSize() {
        QCOMPARE(size({window("zen", 960, 1000), window("terminal", 960, 1000)}, "zen"), std::optional<QSize>(QSize(960, 1000)));
        QCOMPARE(size({window("org.mozilla.Firefox", 1920, 1080)}, "org.mozilla.firefox"), std::optional<QSize>(QSize(1920, 1080)));
        QJsonObject renamed = window("dynamic-class", 800, 600);
        renamed["initialClass"] = "zen";
        QCOMPARE(size({renamed}, "zen"), std::optional<QSize>(QSize(800, 600)));
    }

    void ambiguousMissingAndInvalidWindowsHaveNoHint() {
        QCOMPARE(size({window("zen", 960, 1000), window("zen", 800, 600)}, "zen"), std::nullopt);
        QCOMPARE(size({window("terminal", 960, 1000)}, "zen"), std::nullopt);
        QCOMPARE(size({window("zen", 960, 1000)}, "org.example.zen"), std::nullopt);
        QCOMPARE(size({window("", 960, 1000)}, ""), std::nullopt);
        QCOMPARE(size({}, "zen"), std::nullopt);
        for (const QSize dimensions : {QSize(0, 1000), QSize(960, 0), QSize(-1, 500)}) {
            QCOMPARE(size({window("zen", dimensions.width(), dimensions.height())}, "zen"), std::nullopt);
        }
        for (const auto &[property, value] : {std::pair{"mapped", false}, std::pair{"hidden", true}}) {
            QJsonObject unavailable = window("zen", 960, 1000);
            unavailable[property] = value;
            QCOMPARE(size({unavailable}, "zen"), std::nullopt);
            QCOMPARE(size({unavailable, window("zen", 800, 600)}, "zen"), std::nullopt);
        }
        for (const QByteArray &reply : {QByteArray("not json"), QByteArray("{}"), QByteArray("[{}]")}) {
            QCOMPARE(applicationWindowSize(reply, "zen"), std::nullopt);
        }
    }

    void socketPathsStayInsideTheRuntimeInstance() {
        QCOMPARE(socketPath("/run/user/1000", "abc_123-def"), std::optional<QByteArray>("/run/user/1000/hypr/abc_123-def/.socket.sock"));
        for (const QString &signature : {QString(), QStringLiteral(".."), QStringLiteral("../other"), QStringLiteral("/tmp/socket"),
                                         QStringLiteral("a/b"), QString::fromUtf8("a\0b", 3)}) {
            QCOMPARE(socketPath("/run/user/1000", signature), std::nullopt);
        }
        QCOMPARE(socketPath("relative", "abc"), std::nullopt);
    }

    void compositorQueryIsBoundedAndOptional() {
        QTemporaryDir dir;
        const QByteArray reply = QJsonDocument(QJsonArray{window("zen", 960, 1000)}).toJson();
        QList<QByteArray> asked;
        const auto query = [&](const QByteArray &answer, std::chrono::milliseconds delay,
                               std::chrono::milliseconds timeout) -> std::optional<QSize> {
            const QByteArray path = QFile::encodeName(dir.filePath(QStringLiteral("q%1.sock").arg(++m_sockets)));
            FakeCompositor compositor(path, {answer}, delay);
            const auto result = querySize(path, "zen", timeout);
            asked += compositor.received();
            return result;
        };
        QCOMPARE(query(reply, 0ms, 2s), std::optional<QSize>(QSize(960, 1000)));
        QCOMPARE(query(reply, 60ms, 30ms), std::nullopt);
        QCOMPARE(query(QByteArray(MaxReplyBytes + 1, ' '), 0ms, 2s), std::nullopt);
        QCOMPARE(query("invalid", 0ms, 2s), std::nullopt);
        QCOMPARE(querySize(QFile::encodeName(dir.filePath("missing")), "zen", QueryTimeout), std::nullopt);
        QCOMPARE(asked, QList<QByteArray>(4, "j/clients"));
    }

    void requestsWithoutIdentifiersDoNotQueryTheCompositor() {
        QCOMPARE(parentSizeHint("zen", QString()), std::nullopt);
        QCOMPARE(parentSizeHint("zen", "x11:12"), std::nullopt);
        QCOMPARE(parentSizeHint(QString(), "wayland:abc"), std::nullopt);
    }

    void luaCenteringRuleIsNamedAndMatchesOnlyChooserWindows() {
        QTemporaryDir dir;
        QVERIFY(QByteArray(ChooserClass) != "asc-files");
        QList<QByteArray> first;
        for (int round = 0; round < 2; ++round) {
            const QByteArray path = QFile::encodeName(dir.filePath(QStringLiteral("l%1.sock").arg(round)));
            FakeCompositor compositor(path, {"ok"}, 0ms);
            QVERIFY(installCenteringRule(path, 2s));
            const QList<QByteArray> received = compositor.received();
            QCOMPARE(received, QList<QByteArray>{"/eval hl.window_rule({name='asc-files-chooser-center',match={class='^asc-files-chooser$'},center=true})"});
            if (round == 1) {
                QCOMPARE(received, first);
            }
            first = received;
        }
    }

    void legacyCenteringProbesSupportThenBatchesTheFilterAndEffect() {
        QTemporaryDir dir;
        int round = 0;
        for (const QByteArray &unsupported : {QByteArray("unknown request"), QByteArray("eval is only supported with the lua config manager")}) {
            const QByteArray path = QFile::encodeName(dir.filePath(QStringLiteral("b%1.sock").arg(++round)));
            FakeCompositor compositor(path, {unsupported, "ok", "ok\n\n\nok"}, 0ms);
            QVERIFY(installCenteringRule(path, 2s));
            const QList<QByteArray> received = compositor.received();
            QCOMPARE(received, centeringCommands());
            QCOMPARE(received[1], QByteArray("/keyword windowrule[asc-files-chooser-center]:match:class ^asc-files-chooser$"));
            QCOMPARE(received[2], QByteArray("[[BATCH]] keyword windowrule[asc-files-chooser-center]:match:class ^asc-files-chooser$;"
                                             "keyword windowrule[asc-files-chooser-center]:center on"));
        }
    }

    void placementErrorsStopUnsupportedInstallationAndReportFailedBatches() {
        QTemporaryDir dir;
        int round = 0;
        for (const QList<QByteArray> &replies : QList<QList<QByteArray>>{{"Lua syntax error"},
                                                                         {"unknown request", "invalid field"},
                                                                         {"unknown request", "ok", "invalid effect"},
                                                                         {"unknown request", "ok", "invalid field\n\n\nok"},
                                                                         {"unknown request", "ok", "ok\n\n\ninvalid effect"}}) {
            const QByteArray path = QFile::encodeName(dir.filePath(QStringLiteral("e%1.sock").arg(++round)));
            FakeCompositor compositor(path, replies, 0ms);
            QVERIFY(!installCenteringRule(path, 2s));
            QCOMPARE(compositor.received().size(), replies.size());
        }
    }

    void placementIpcHasATotalDeadlineAndReplyLimit() {
        QTemporaryDir dir;
        {
            const QByteArray path = QFile::encodeName(dir.filePath("d.sock"));
            FakeCompositor compositor(path, {"ok"}, 60ms);
            QVERIFY(!installCenteringRule(path, 30ms));
        }
        {
            const QByteArray path = QFile::encodeName(dir.filePath("o.sock"));
            FakeCompositor compositor(path, {QByteArray(8193, ' ')}, 0ms);
            QVERIFY(!installCenteringRule(path, 2s));
        }
        QVERIFY(!installCenteringRule(QFile::encodeName(dir.filePath("missing")), QueryTimeout));
    }

private:
    int m_sockets = 0;
};

QTEST_GUILESS_MAIN(TestPlacement)
#include "tst_placement.moc"
