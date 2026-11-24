// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "models/PlacesModel.h"

#include <QAbstractItemModelTester>
#include <QTemporaryDir>

namespace {

QStringList paths(const PlacesModel &places, PlacesModel::Section section) {
    QStringList result;
    for (int row = 0; row < places.rowCount(); ++row) {
        const QModelIndex index = places.index(row);
        if (places.data(index, PlacesModel::SectionRole).toInt() == section) {
            result.append(places.data(index, PlacesModel::PathRole).toString());
        }
    }
    return result;
}

} // namespace

class TestPlaces : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void homeAndTheSystemDriveAreAlwaysThere() {
        PlacesModel places;
        QAbstractItemModelTester tester(&places, QAbstractItemModelTester::FailureReportingMode::QtTest);
        QCOMPARE(paths(places, PlacesModel::Folders).first(), QDir::homePath());
        QCOMPARE(paths(places, PlacesModel::Storage).first(), QStringLiteral("/"));
        const QModelIndex system = places.index(places.rowCount() - int(paths(places, PlacesModel::Storage).size()));
        const double used = places.data(system, PlacesModel::UsedRole).toDouble();
        QVERIFY(used >= 0 && used <= 1);
        QVERIFY(places.data(system, PlacesModel::FreeTextRole).toString().contains(QStringLiteral("free")));
        places.refreshDrives();
    }

    void pinnedFoldersAreKept() {
        QTemporaryDir dir;
        {
            PlacesModel places;
            QAbstractItemModelTester tester(&places, QAbstractItemModelTester::FailureReportingMode::QtTest);
            places.pin(dir.path());
            places.pin(dir.path());
            QCOMPARE(paths(places, PlacesModel::Pinned), QStringList{dir.path()});
            QVERIFY(places.isPinned(dir.path()));
        }
        PlacesModel again;
        QCOMPARE(paths(again, PlacesModel::Pinned), QStringList{dir.path()});
        again.unpin(dir.path());
        QVERIFY(paths(again, PlacesModel::Pinned).isEmpty());
    }
};

QTEST_GUILESS_MAIN(TestPlaces)
#include "tst_places.moc"
