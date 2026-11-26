// SPDX-License-Identifier: MIT
//! Whether a newer Files is out (no network: GitHub's answer as it would come).
#include "desktop/Updates.h"

#include <QJsonDocument>
#include <QTest>

class TestUpdates : public QObject {
    Q_OBJECT
private slots:
    void versionsCompareAsReleasesDo() {
        QCOMPARE(Updates::compareVersions("0.1.0", "0.1.0"), 0);
        QCOMPARE(Updates::compareVersions("v0.2.0", "0.1.9"), 1);
        QCOMPARE(Updates::compareVersions("0.10.0", "0.9.0"), 1);
        QCOMPARE(Updates::compareVersions("1.0", "1.0.0"), 0);
        QCOMPARE(Updates::compareVersions("1.0.0-beta.1", "1.0.0"), -1);
        QCOMPARE(Updates::compareVersions("1.0.0-beta.2", "1.0.0-beta.10"), -1);
        QCOMPARE(Updates::compareVersions("1.0.0-rc.1", "1.0.0-beta.3"), 1);
    }

    void theNewestReleaseOnTheChannelIsOffered() {
        const QJsonArray releases = QJsonDocument::fromJson(
            "["
            "{\"tag_name\": \"v0.3.0-beta.1\", \"prerelease\": true, \"draft\": false, \"html_url\": \"https://x/beta\"},"
            "{\"tag_name\": \"v0.2.1\", \"prerelease\": false, \"draft\": false, \"html_url\": \"https://x/021\"},"
            "{\"tag_name\": \"v0.4.0\", \"prerelease\": false, \"draft\": true, \"html_url\": \"https://x/draft\"},"
            "{\"tag_name\": \"v0.2.0\", \"prerelease\": false, \"draft\": false, \"html_url\": \"https://x/020\"},"
            "{\"tag_name\": \"\", \"prerelease\": false},"
            "\"nonsense\""
            "]").array();
        QVariantMap found = Updates::newest(releases, "0.1.0", "stable");
        QCOMPARE(found.value("version").toString(), QStringLiteral("0.2.1"));
        QCOMPARE(found.value("url").toString(), QStringLiteral("https://x/021"));
        QCOMPARE(Updates::newest(releases, "0.1.0", "beta").value("version").toString(), QStringLiteral("0.3.0-beta.1"));
        // Drafts never; nothing when this is the newest.
        QVERIFY(Updates::newest(releases, "0.2.1", "stable").isEmpty());
        QVERIFY(Updates::newest({}, "0.1.0", "stable").isEmpty());
    }

    void aCheckIsDueOnceADayWhenAllowed() {
        const QDateTime now = QDateTime::currentDateTime();
        QVERIFY(Updates::due(true, {}, now));
        QVERIFY(!Updates::due(false, {}, now));
        QVERIFY(!Updates::due(true, now.addSecs(-3600), now));
        QVERIFY(Updates::due(true, now.addSecs(-Updates::DaySeconds), now));
        // A clock set back doesn't stop checks for good.
        QVERIFY(Updates::due(true, now.addDays(30), now));
    }
};

QTEST_GUILESS_MAIN(TestUpdates)
#include "tst_updates.moc"
