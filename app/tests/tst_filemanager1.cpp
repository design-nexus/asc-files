// SPDX-License-Identifier: MIT
//! Legacy's FileManager1 tests (`adapters/file_manager1/tests.rs`), on the requests a call makes;
//! the remote-URI one waits for remote locations (B7).
#include "desktop/DesktopBus.h"

#include <QSignalSpy>
#include <QTest>

using Method = FileManager1::Method;

class TestFileManager1 : public QObject {
    Q_OBJECT
private slots:
    void showItemsRevealsTheParentDirectoryWithTheItemSelected() {
        const QList<Reveal> requests = FileManager1::revealRequests(Method::Items, {"file:///home/user/Downloads/example.md"});
        QCOMPARE(requests.size(), 1);
        QCOMPARE(requests[0].folder, QByteArray("/home/user/Downloads"));
        QCOMPARE(requests[0].items, QList<QByteArray>{"/home/user/Downloads/example.md"});
        QVERIFY(!requests[0].properties);
    }

    void showFoldersOpensTheNamedDirectoriesWithoutASelection() {
        const QList<Reveal> requests = FileManager1::revealRequests(Method::Folders, {"file:///home/user/Downloads"});
        QCOMPARE(requests.size(), 1);
        QCOMPARE(requests[0].folder, QByteArray("/home/user/Downloads"));
        QVERIFY(requests[0].items.isEmpty());
    }

    void itemsSharingADirectoryAreSelectedTogetherInOneWindow() {
        const QList<Reveal> requests = FileManager1::revealRequests(
            Method::Items, {"file:///home/user/Downloads/first.md", "file:///home/user/Pictures/photo.png", "file:///home/user/Downloads/second.md"});
        QCOMPARE(requests.size(), 2);
        QCOMPARE(requests[0].folder, QByteArray("/home/user/Downloads"));
        QCOMPARE(requests[0].items, (QList<QByteArray>{"/home/user/Downloads/first.md", "/home/user/Downloads/second.md"}));
        QCOMPARE(requests[1].folder, QByteArray("/home/user/Pictures"));
        QCOMPARE(requests[1].items, QList<QByteArray>{"/home/user/Pictures/photo.png"});
    }

    void showItemPropertiesMarksTheRequest() {
        const QList<Reveal> requests = FileManager1::revealRequests(Method::ItemProperties, {"file:///home/user/Downloads/example.md"});
        QCOMPARE(requests, (QList<Reveal>{{"/home/user/Downloads", {"/home/user/Downloads/example.md"}, true}}));
    }

    void aRootWithoutAParentIsOpenedDirectly() {
        const QList<Reveal> requests = FileManager1::revealRequests(Method::Items, {"file:///"});
        QCOMPARE(requests, (QList<Reveal>{{"/", {}, false}}));
    }

    void showItemsKeepsCollidingNonUtf8NamesDistinct() {
        const QList<Reveal> requests = FileManager1::revealRequests(
            Method::Items, {"file:///home/user/Downloads/bad%E8name.txt", "file:///home/user/Downloads/bad%E9name.txt"});
        QCOMPARE(requests.size(), 1);
        QCOMPARE(requests[0].items, (QList<QByteArray>{"/home/user/Downloads/bad\xe8name.txt", "/home/user/Downloads/bad\xe9name.txt"}));
    }

    // New: what isn't a local file is left out, for now; the calls answer through `reveal`.
    void onlyLocalUrisAreShownAndEachCallAnswers() {
        QVERIFY(FileManager1::revealRequests(Method::Items, {"sftp://host/home/user/notes.txt", "not a uri", "file://elsewhere/x"}).isEmpty());
        FileManager1 manager;
        QSignalSpy reveals(&manager, &FileManager1::reveal);
        manager.ShowItems({"file:///tmp/a.txt", "file:///var/b.txt"}, {});
        manager.ShowFolders({"file:///tmp/"}, {});
        manager.ShowItemProperties({"file:///tmp/a.txt"}, {});
        QCOMPARE(reveals.size(), 4);
        QCOMPARE(reveals.at(2).at(0).value<Reveal>().folder, QByteArray("/tmp"));
        QVERIFY(reveals.at(3).at(0).value<Reveal>().properties);
    }
};

QTEST_GUILESS_MAIN(TestFileManager1)
#include "tst_filemanager1.moc"
