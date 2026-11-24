// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FavoritesModel.h"
#include "models/JobModel.h"
#include "models/RecentModel.h"
#include "models/TrashModel.h"

#include <QAbstractItemModelTester>
#include <QFile>
#include <QJsonArray>
#include <QSaveFile>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QUrl>

namespace {

QStringList column(const ItemListModel &model, int role) {
    QStringList result;
    for (int row = 0; row < model.rowCount(); ++row) {
        result.append(model.data(model.index(row), role).toString());
    }
    return result;
}

void write(const QString &path, const QByteArray &contents) {
    QDir().mkpath(QFileInfo(path).path());
    QSaveFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
    QVERIFY(file.commit());
}

} // namespace

class TestItemLists : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void recentListsLocalFilesNewestFirst() {
        QTemporaryDir dir;
        write(dir.filePath("old.txt"), "o");
        write(dir.filePath("new.txt"), "new");
        const auto url = [&](const char *name) { return QUrl::fromLocalFile(dir.filePath(QString::fromLatin1(name))).toEncoded(); };
        write(RecentModel::recordPath(),
              "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n"
              "<bookmark href=\"" + url("old.txt") + "\" added=\"2026-01-01T00:00:00Z\" modified=\"2026-01-01T00:00:00Z\" visited=\"2026-01-01T00:00:00Z\"/>\n"
              "<bookmark href=\"https://example.com/x\" modified=\"2026-11-01T00:00:00Z\" visited=\"2026-11-01T00:00:00Z\"/>\n"
              "<bookmark href=\"" + url("new.txt") + "\" added=\"2026-02-01T00:00:00Z\" modified=\"2026-11-20T10:00:00Z\" visited=\"2026-11-20T10:00:00Z\">"
              "<info><metadata owner=\"http://freedesktop.org\"/></info></bookmark>\n"
              "</xbel>\n");
        RecentModel recent;
        QAbstractItemModelTester tester(&recent, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QCOMPARE(column(recent, ItemListModel::NameRole), (QStringList{"new.txt", "old.txt"}));
        QTRY_COMPARE(recent.data(recent.index(0), ItemListModel::SizeTextRole).toString().isEmpty(), false);
        QCOMPARE(recent.data(recent.index(0), ItemListModel::ExistsRole).toBool(), true);

        // A program adding to the record is followed.
        QSignalSpy count(&recent, &ItemListModel::countChanged);
        write(RecentModel::recordPath(), "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n</xbel>\n");
        QTRY_COMPARE(recent.rowCount(), 0);
    }

    void favoritesAreKeptAndLegacyOnesTaken() {
        QTemporaryDir dir;
        write(dir.filePath("starred.txt"), "s");
        QFile::remove(FavoritesModel::filePath());
        const QString legacy = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + "/nyx-files/favorites";
        write(legacy, QUrl::fromLocalFile(dir.path()).toEncoded() + "\nsftp://host/far\n");
        {
            FavoritesModel favorites;
            QAbstractItemModelTester tester(&favorites, QAbstractItemModelTester::FailureReportingMode::QtTest);
            QCOMPARE(column(favorites, ItemListModel::PathRole), QStringList{dir.path()});
            favorites.add(dir.filePath("starred.txt"));
            favorites.add(dir.filePath("starred.txt"));
            QCOMPARE(favorites.rowCount(), 2);
            QVERIFY(favorites.contains(dir.filePath("starred.txt")));
            QTRY_VERIFY(favorites.isFolderAt(0));
        }
        FavoritesModel again;
        QCOMPARE(again.rowCount(), 2);
        again.remove(dir.path());
        QCOMPARE(column(again, ItemListModel::PathRole), QStringList{dir.filePath("starred.txt")});
        // Legacy's own file is left as it was.
        QFile legacyFile(legacy);
        QVERIFY(legacyFile.open(QIODevice::ReadOnly));
        QVERIFY(legacyFile.readAll().contains("sftp://host/far"));
        QFile::remove(FavoritesModel::filePath());
        QFile::remove(legacy);
    }

    void trashedItemsShowWhereTheyCameFrom() {
        QTemporaryDir dir(QDir::home().filePath(QStringLiteral("trashing-XXXXXX")));
        write(dir.filePath("doomed.txt"), "bye");
        BackendClient backend(TestSupport::backend());
        backend.start();
        TrashModel trash(&backend);
        JobModel jobs(&backend);
        QSignalSpy finished(&jobs, &JobModel::finished);
        jobs.start({{"into", dir.path()},
                    {"items", QJsonArray{QJsonObject{{"source", dir.filePath("doomed.txt")}, {"action", "trash"}}}}},
                   QStringLiteral("Move to Trash"));
        QVERIFY(finished.wait(20000));
        QTRY_VERIFY_WITH_TIMEOUT(column(trash, ItemListModel::NameRole).contains("doomed.txt"), 10000);
        const int row = int(column(trash, ItemListModel::NameRole).indexOf("doomed.txt"));
        QCOMPARE(trash.originalAt(row), dir.filePath("doomed.txt"));
        QVERIFY(trash.data(trash.index(row), ItemListModel::ExtraRole).toString().endsWith(".trashinfo"));
        QVERIFY(!trash.data(trash.index(row), ItemListModel::WhenTextRole).toString().isEmpty());
        const QString shown = trash.data(trash.index(row), ItemListModel::FolderRole).toString();
        QVERIFY2(shown.startsWith(u'~') && shown.endsWith(QFileInfo(dir.path()).fileName()), qPrintable(shown));
        // Leave nothing in the private Trash: delete the entry the test made.
        bool deleted = false;
        backend.call(QStringLiteral("trash.delete"), {{"info", trash.data(trash.index(row), ItemListModel::ExtraRole).toString()}},
                     [&](const QJsonValue &, const BackendError &error) { deleted = error.isNull(); });
        QTRY_VERIFY(deleted);
    }
};

QTEST_GUILESS_MAIN(TestItemLists)
#include "tst_itemlists.moc"
