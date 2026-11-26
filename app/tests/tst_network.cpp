// SPDX-License-Identifier: MIT
//! Network locations: which text is a server address, and (by hand, with gvfs) connecting.
#include "desktop/Network.h"

#include <QDir>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

class TestNetwork : public QObject {
    Q_OBJECT
private slots:
    void addressesAreToldFromFolders() {
        for (const char *address : {"sftp://host/home", "smb://server/share", "davs://cloud.example/remote.php", "ftp://host"}) {
            QVERIFY2(Network::isAddress(address), address);
        }
        for (const char *folder : {"/home/user", "~/Documents", "file:///tmp", "Downloads", "smb:", "://nothing", "sftp://"}) {
            QVERIFY2(!Network::isAddress(folder), folder);
        }
    }

    // By hand (ASC_FILES_LIVE_NETWORK=1): through gvfs's local test backend, connected, shown as
    // a folder, listed, and disconnected.
    void aServerIsConnectedOpenedAndDisconnected() {
        if (qEnvironmentVariable("ASC_FILES_LIVE_NETWORK") != QLatin1String("1")) {
            QSKIP("set ASC_FILES_LIVE_NETWORK=1 to connect through gvfs");
        }
        QTemporaryDir dir;
        QVERIFY(QDir(dir.path()).mkdir("inside"));
        Network network;
        QSignalSpy opened(&network, &Network::opened);
        QSignalSpy failed(&network, &Network::failed);
        network.connectTo(QStringLiteral("localtest://") + dir.path());
        QVERIFY2(opened.wait(15000), qPrintable(failed.isEmpty() ? QString() : failed.first().at(0).toString()));
        const QString path = opened.first().at(0).toString();
        QVERIFY2(path.contains("/gvfs/"), qPrintable(path));
        QVERIFY(QDir(path).entryList(QDir::Dirs | QDir::NoDotAndDotDot).contains("inside"));
        QTRY_VERIFY(!network.mounts().isEmpty());
        QString uri;
        for (const QVariant &mount : network.mounts()) {
            if (mount.toMap().value("path").toString() == path || path.startsWith(mount.toMap().value("path").toString())) {
                uri = mount.toMap().value("uri").toString();
            }
        }
        QVERIFY(!uri.isEmpty());
        network.disconnectFrom(uri);
        QTRY_VERIFY_WITH_TIMEOUT(!QDir(path).exists(), 10000);
    }
};

QTEST_GUILESS_MAIN(TestNetwork)
#include "tst_network.moc"
