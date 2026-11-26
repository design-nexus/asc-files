// SPDX-License-Identifier: MIT
//! Cloud accounts as Nyx Cloud lists them.
#include "desktop/Cloud.h"

#include <QTest>

class TestCloud : public QObject {
    Q_OBJECT
private slots:
    void accountsWithStorageAreListed() {
        const QByteArray json = R"([
            {"id": "a1", "name": "Google Drive", "provider": "drive", "connection": "connected", "mount_status": "mounted",
             "mount_path": "/home/user/Google Drive", "storage_enabled": true, "error": "", "later": {"field": 1}},
            {"id": "b2", "name": "Work", "provider": "onedrive", "mount_status": "unmounted", "mount_path": "/home/user/Work",
             "storage_enabled": true, "error": "Token expired"},
            {"id": "c3", "name": "Mail only", "provider": "drive", "mount_status": "unmounted", "storage_enabled": false},
            {"name": "No id"}
        ])";
        const QVariantList accounts = Cloud::accountsFrom(json);
        QCOMPARE(accounts.size(), 2);
        const QVariantMap drive = accounts.at(0).toMap();
        QCOMPARE(drive.value("name").toString(), QStringLiteral("Google Drive"));
        QVERIFY(drive.value("mounted").toBool());
        QCOMPARE(drive.value("path").toString(), QStringLiteral("/home/user/Google Drive"));
        const QVariantMap work = accounts.at(1).toMap();
        QVERIFY(!work.value("mounted").toBool());
        QCOMPARE(work.value("error").toString(), QStringLiteral("Token expired"));
    }

    void nothingReadableIsNoAccounts() {
        QVERIFY(Cloud::accountsFrom("").isEmpty());
        QVERIFY(Cloud::accountsFrom("{\"not\": \"a list\"}").isEmpty());
        QVERIFY(Cloud::accountsFrom("[1, \"two\", null]").isEmpty());
    }
};

QTEST_GUILESS_MAIN(TestCloud)
#include "tst_cloud.moc"
