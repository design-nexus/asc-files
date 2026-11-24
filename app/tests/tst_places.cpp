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

    void defaultPlacesHideAndComeBack() {
        PlacesModel places;
        QAbstractItemModelTester tester(&places, QAbstractItemModelTester::FailureReportingMode::QtTest);
        const auto keys = [&] {
            QStringList result;
            for (int row = 0; row < places.rowCount(); ++row) {
                result.append(places.data(places.index(row), PlacesModel::KeyRole).toString());
            }
            return result;
        };
        QVERIFY(keys().contains("trash"));
        places.setHidden("trash", true);
        places.setHidden("recent", true);
        QVERIFY(!keys().contains("trash") && !keys().contains("recent"));
        QCOMPARE(places.hiddenPlaces().size(), 2);
        // Drives are still followed with the Trash hidden.
        places.refreshDrives();
        QCOMPARE(keys().last().startsWith("drive:"), true);
        // Drives and pinned folders cannot be hidden this way.
        places.setHidden("drive:/", true);
        QVERIFY(keys().contains("drive:/"));
        {
            PlacesModel again;
            QCOMPARE(again.hiddenPlaces().size(), 2);
        }
        places.setHidden("trash", false);
        places.setHidden("recent", false);
        QVERIFY(keys().contains("trash") && keys().contains("recent"));
        QCOMPARE(places.hiddenPlaces().size(), 0);
    }

    void foldersGoInTheOrderGiven() {
        QDir home = QDir::home();
        for (const char *folder : {"Desktop", "Documents", "Downloads"}) {
            home.mkpath(QString::fromLatin1(folder));
        }
        PlacesModel places;
        const auto folders = [&] {
            QStringList result;
            for (int row = 0; row < places.rowCount(); ++row) {
                if (places.data(places.index(row), PlacesModel::SectionRole).toInt() == PlacesModel::Folders) {
                    result.append(places.data(places.index(row), PlacesModel::KeyRole).toString());
                }
            }
            return result;
        };
        const QStringList before = folders();
        QVERIFY2(before.size() >= 3, qPrintable(before.join(',')));
        places.moveFolder(before.last(), before.first());
        QCOMPARE(folders().first(), before.last());
        {
            PlacesModel again;
            QCOMPARE(again.data(again.index(3), PlacesModel::KeyRole).toString(), before.last());
        }
        places.resetFolderOrder();
        QCOMPARE(folders(), before);
    }

    void theSidebarStepsAndKeepsItsColours() {
        PlacesModel places;
        QCOMPARE(places.mode(), int(PlacesModel::Full));
        places.stepMode();
        QCOMPARE(places.mode(), int(PlacesModel::Compact));
        places.stepMode();
        QCOMPARE(places.mode(), int(PlacesModel::Hidden));
        {
            PlacesModel again;
            QCOMPARE(again.mode(), int(PlacesModel::Hidden));
        }
        places.stepMode();
        QCOMPARE(places.mode(), int(PlacesModel::Full));
        places.setSectionColor(PlacesModel::Pinned, "green");
        QCOMPARE(places.sectionColors().value("2").toString(), QStringLiteral("green"));
        places.setSectionColor(PlacesModel::Pinned, "");
        QVERIFY(places.sectionColors().isEmpty());
        places.setColoredIcons(false);
        QVERIFY(!PlacesModel().coloredIcons());
        places.setColoredIcons(true);
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
