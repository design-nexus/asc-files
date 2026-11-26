// SPDX-License-Identifier: MIT
//! English plural forms: one item, two items.
#include <QCoreApplication>
#include <QTest>
#include <QTranslator>

class TestTranslations : public QObject {
    Q_OBJECT
private slots:
    void englishCountsOneAndMany() {
        QTranslator english;
        QVERIFY(english.load(QStringLiteral("asc-files_en"), QStringLiteral(ASC_FILES_QM_DIR)));
        QCoreApplication::installTranslator(&english);
        QCOMPARE(QCoreApplication::translate("Operations", "Cut %n items", nullptr, 1), QStringLiteral("Cut 1 item"));
        QCOMPARE(QCoreApplication::translate("Operations", "Cut %n items", nullptr, 3), QStringLiteral("Cut 3 items"));
        QCOMPARE(QCoreApplication::translate("Operations", "Cut %n items", nullptr, 0), QStringLiteral("Cut 0 items"));
        QCOMPARE(QCoreApplication::translate("ArchiveView", "%n files", nullptr, 1), QStringLiteral("1 file"));
        QCOMPARE(QCoreApplication::translate("ArchiveView", "%n folders", nullptr, 2), QStringLiteral("2 folders"));
        QCoreApplication::removeTranslator(&english);
    }
};

QTEST_GUILESS_MAIN(TestTranslations)
#include "tst_translations.moc"
