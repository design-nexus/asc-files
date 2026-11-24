// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "backend/BackendClient.h"
#include "models/FolderModel.h"

#include <QAbstractItemModelTester>
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <csignal>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

namespace {

/// Makes a file at raw `path`: QFile would turn a name that is not UTF-8 into another.
void touch(const QByteArray &path, const QByteArray &contents = {}) {
    const int fd = ::open(path.constData(), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    QVERIFY(fd >= 0);
    QCOMPARE(::write(fd, contents.constData(), size_t(contents.size())), ssize_t(contents.size()));
    ::close(fd);
}

/// Waits until every row of `model` has arrived, asking for each as a view would.
bool loadAll(FolderModel &model) {
    for (int row = 0; row < model.rowCount(); ++row) {
        model.data(model.index(row), FolderModel::NameRole);
    }
    return QTest::qWaitFor([&] {
        for (int row = 0; row < model.rowCount(); ++row) {
            if (!model.isLoaded(row)) {
                return false;
            }
        }
        return true;
    }, 10000);
}

QStringList names(FolderModel &model) {
    QStringList result;
    for (int row = 0; row < model.rowCount(); ++row) {
        result.append(model.data(model.index(row), FolderModel::NameRole).toString());
    }
    return result;
}

} // namespace

class TestFolderModel : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void listsAFolderAPageAtATime() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        for (int i = 0; i < 450; ++i) {
            touch(root + "/f" + QByteArray::number(i).rightJustified(3, '0') + ".txt", "x");
        }
        QDir().mkdir(dir.filePath("sub"));
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.rowCount(), 451);
        QCOMPARE(model.pagesFetched(), 0);
        // Folders first, then by name.
        QCOMPARE(model.data(model.index(0), FolderModel::LoadedRole).toBool(), false);
        QTRY_VERIFY(model.isLoaded(0));
        QCOMPARE(model.data(model.index(0), FolderModel::NameRole).toString(), QStringLiteral("sub"));
        QCOMPARE(model.data(model.index(0), FolderModel::IsFolderRole).toBool(), true);
        QCOMPARE(model.data(model.index(1), FolderModel::NameRole).toString(), QStringLiteral("f000.txt"));
        QCOMPARE(model.data(model.index(1), FolderModel::SizeTextRole).toString(), QLocale().formattedDataSize(1, 1, QLocale::DataSizeSIFormat));
        QCOMPARE(model.data(model.index(1), FolderModel::KindTextRole).toString().isEmpty(), false);
        QVERIFY(!model.isLoaded(300));
        QVERIFY(loadAll(model));
        QCOMPARE(model.pagesFetched(), 3);
    }

    void namesThatAreNotUtf8OpenAndShow() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        const QByteArray odd = root + "/caf\xe9";
        QCOMPARE(::mkdir(odd.constData(), 0755), 0);
        touch(odd + "/inside.txt");
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QVERIFY(loadAll(model));
        QCOMPARE(model.rawName(0), QByteArray("caf\xe9"));
        QCOMPARE(model.data(model.index(0), FolderModel::NameRole).toString(), QStringLiteral("caf�"));
        model.openRow(0);
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.rawFolder(), odd);
        QVERIFY(loadAll(model));
        QCOMPARE(names(model), QStringList{QStringLiteral("inside.txt")});
        model.openParent();
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.rawFolder(), root);
    }

    void followsTheFolderWithoutResetting() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        touch(root + "/a.txt");
        touch(root + "/b.txt");
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QVERIFY(model.followsChanges());
        QVERIFY(loadAll(model));
        QSignalSpy reset(&model, &QAbstractItemModel::modelReset);
        QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
        QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);

        touch(root + "/c.txt");
        QTRY_COMPARE_WITH_TIMEOUT(model.rowCount(), 3, 10000);
        QCOMPARE(inserted.count(), 1);
        QVERIFY(loadAll(model));
        QCOMPARE(names(model), (QStringList{"a.txt", "b.txt", "c.txt"}));

        QVERIFY(QFile::remove(dir.filePath("a.txt")));
        QTRY_COMPARE_WITH_TIMEOUT(model.rowCount(), 2, 10000);
        QCOMPARE(removed.count(), 1);
        QVERIFY(loadAll(model));
        QCOMPARE(names(model), (QStringList{"b.txt", "c.txt"}));
        QCOMPARE(reset.count(), 0);
    }

    void aFolderThatGoesTakesTheModelUp() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        QVERIFY(QDir().mkpath(dir.filePath("doomed/deeper")));
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        QSignalSpy left(&model, &FolderModel::folderLeft);
        model.open(root + "/doomed/deeper");
        QVERIFY(opened.wait(10000));
        QVERIFY(QDir(dir.filePath("doomed")).removeRecursively());
        QVERIFY(left.wait(10000));
        QCOMPARE(left.first().at(2).toBool(), false);
        QTRY_COMPARE(model.rawFolder(), root);
    }

    void aMissingFolderReportsItsKind() {
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy error(&model, &FolderModel::errorChanged);
        model.openPath(QStringLiteral("/no/such/folder/here"));
        QVERIFY(error.wait(10000));
        QCOMPARE(model.errorKind(), QStringLiteral("notFound"));
        QCOMPARE(model.rowCount(), 0);
    }

    void aRestartedBackendListsTheFolderAgain() {
        QTemporaryDir dir;
        touch(QFile::encodeName(dir.path()) + "/kept.txt");
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.openPath(dir.path());
        QVERIFY(opened.wait(10000));
        ::kill(pid_t(backend.processId()), SIGKILL);
        QVERIFY(opened.wait(10000));
        QVERIFY(loadAll(model));
        QCOMPARE(names(model), QStringList{QStringLiteral("kept.txt")});
    }

    void selectionIsKeptByName() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        for (int i = 0; i < 500; ++i) {
            touch(root + "/f" + QByteArray::number(i).rightJustified(3, '0'));
        }
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        model.data(model.index(0), FolderModel::NameRole);
        QTRY_VERIFY(model.isLoaded(0));
        model.select(2);
        QCOMPARE(model.selectionCount(), 1);
        model.toggle(4);
        QCOMPARE(model.selectionCount(), 2);
        model.toggle(2);
        QCOMPARE(model.selectedNames(), QList<QByteArray>{"f004"});
        // A range, from the last row clicked (2), reaching rows not read yet: they fill in as
        // their pages come.
        model.extendTo(450);
        QCOMPARE(model.selectionCount(), 449);
        QTRY_COMPARE(model.selectedNames().size(), 449);
        QVERIFY(model.isSelected(300));
        // Everything, then all but one, without loading every name.
        model.selectAll();
        QCOMPARE(model.selectionCount(), 500);
        model.toggle(0);
        QCOMPARE(model.selectionCount(), 499);
        QVERIFY(!model.isSelected(0));
        model.clearSelection();
        QCOMPARE(model.selectionCount(), 0);

        // A new file shifts the rows; the selection follows the names.
        model.select(10);
        QCOMPARE(model.selectedNames(), QList<QByteArray>{"f010"});
        touch(root + "/a-first");
        QTRY_COMPARE_WITH_TIMEOUT(model.rowCount(), 501, 10000);
        model.data(model.index(11), FolderModel::NameRole);
        QTRY_VERIFY(model.isLoaded(11));
        QVERIFY(model.isSelected(11));
        QVERIFY(!model.isSelected(10));
        // Opening another folder clears it.
        model.open(root + "/..");
        QVERIFY(opened.wait(10000));
        QCOMPARE(model.selectionCount(), 0);
    }

    void sortingReadsTheFolderInAnotherOrder() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        touch(root + "/b-small", "x");
        touch(root + "/a-large", QByteArray(5000, 'x'));
        touch(root + "/c-medium", QByteArray(100, 'x'));
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QVERIFY(loadAll(model));
        QCOMPARE(names(model), (QStringList{"a-large", "b-small", "c-medium"}));
        model.setSort(QStringLiteral("size"), true);
        QTRY_VERIFY(loadAll(model) && names(model) == (QStringList{"a-large", "c-medium", "b-small"}));
        QCOMPARE(model.sortKey(), QStringLiteral("size"));
        model.setSort(QStringLiteral("name"), true);
        QTRY_VERIFY(loadAll(model) && names(model) == (QStringList{"c-medium", "b-small", "a-large"}));
    }

    void renamingKeepsTheItemSelected() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        touch(root + "/draft.txt");
        touch(root + "/taken.txt");
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        QSignalSpy renamed(&model, &FolderModel::renamed);
        QSignalSpy failed(&model, &FolderModel::failed);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QVERIFY(loadAll(model));
        model.select(0);
        model.rename(0, QStringLiteral("final.txt"));
        QVERIFY(renamed.wait(10000));
        QVERIFY(QFile::exists(dir.filePath("final.txt")));
        QCOMPARE(model.selectedNames(), QList<QByteArray>{"final.txt"});
        QTRY_VERIFY(loadAll(model) && names(model).contains("final.txt"));
        const int row = int(names(model).indexOf("final.txt"));
        QVERIFY(model.isSelected(row));
        // A name that is taken is refused, with the reason.
        model.rename(row, QStringLiteral("taken.txt"));
        QVERIFY(failed.wait(10000));
        QVERIFY2(failed.first().first().toString().contains("already exists"), qPrintable(failed.first().first().toString()));
        QVERIFY(QFile::exists(dir.filePath("final.txt")));
    }

    void modesShowOctalAndSymbolic() {
        QTemporaryDir dir;
        const QByteArray root = QFile::encodeName(dir.path());
        touch(root + "/script.sh");
        QFile::setPermissions(dir.filePath("script.sh"), QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner |
                                                             QFile::ReadGroup | QFile::ExeGroup | QFile::ReadOther | QFile::ExeOther);
        BackendClient backend(TestSupport::backend());
        backend.start();
        FolderModel model(&backend);
        QSignalSpy opened(&model, &FolderModel::opened);
        model.open(root);
        QVERIFY(opened.wait(10000));
        QVERIFY(loadAll(model));
        QCOMPARE(model.data(model.index(0), FolderModel::ModeOctalRole).toString(), QStringLiteral("755"));
        QCOMPARE(model.data(model.index(0), FolderModel::ModeSymbolicRole).toString(), QStringLiteral("-rwxr-xr-x"));
    }
};

QTEST_GUILESS_MAIN(TestFolderModel)
#include "tst_foldermodel.moc"
