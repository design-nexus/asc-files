// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "theme/SystemTheme.h"

#include <QDir>
#include <QFile>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QSignalSpy>

namespace {

void writeAtomically(const QString &path, const QByteArray &contents) {
    QDir().mkpath(QFileInfo(path).path());
    QSaveFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
    QVERIFY(file.commit());
}

QByteArray snapshot(const char *name, const char *appearance, const char *surface, const char *text, const char *filesAccent) {
    QJsonObject colors{{"surface.primary", surface}, {"text.primary", text}, {"accent.primary", "#123456"}};
    return QJsonDocument(QJsonObject{
                             {"format", 1},
                             {"generation", 7},
                             {"theme", QJsonObject{{"id", name}, {"name", name}, {"appearance", appearance}}},
                             {"tokens", QJsonObject{{"colors", colors},
                                                    {"typography", QJsonObject{{"ui", "No Such Font Sans, Inter"}, {"mono", "No Such Font Mono"}}},
                                                    {"geometry", QJsonObject{{"radius_small", 4}, {"radius_card", 8}, {"radius_dialog", 12}}}}},
                             {"swatches", QJsonObject{{"amber", "#aa8800"}, {"violet", "#7755cc"}, {"green", "#22aa44"}}},
                             {"accents", QJsonObject{{"files", filesAccent}}},
                         })
        .toJson();
}

} // namespace

class TestSystemTheme : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        QVERIFY(SystemTheme::runtimeDir().contains("asc-files-test-home"));
        QFile::remove(SystemTheme::runtimeDir() + "/system.json");
        QFile::remove(SystemTheme::suitePath());
    }

    void withoutASnapshotNyxDarkIsUsed() {
        SystemTheme theme;
        QCOMPARE(theme.themeName(), QStringLiteral("Nyx Dark"));
        QCOMPARE(theme.color("surface.primary"), QColor("#0d0e12"));
        QCOMPARE(theme.accent(), QColor("#3d8ae8"));
        QVERIFY(!theme.light());
        QCOMPARE(theme.textScale(), 1.0);
        QCOMPARE(theme.density(), QStringLiteral("comfortable"));
    }

    void aThemeSwitchedReachesFilesWhole() {
        writeAtomically(SystemTheme::runtimeDir() + "/system.json", snapshot("Paper", "light", "#fafafa", "#202020", "#cc3355"));
        SystemTheme theme;
        QCOMPARE(theme.themeName(), QStringLiteral("Paper"));
        QVERIFY(theme.light());
        QCOMPARE(theme.color("surface.primary"), QColor("#fafafa"));
        // Tokens the snapshot leaves out keep Nyx Dark's.
        QCOMPARE(theme.color("status.error"), QColor("#f2655f"));
        QCOMPARE(theme.accent(), QColor("#cc3355"));
        // Families the computer lacks: the system's own, fixed-width for code.
        QCOMPARE(theme.family(), QFontDatabase::systemFont(QFontDatabase::GeneralFont).family());
        QCOMPARE(theme.monoFamily(), QFontDatabase::systemFont(QFontDatabase::FixedFont).family());
        QCOMPARE(theme.radiusSmall(), 4);

        // Switched while running: followed.
        QSignalSpy changed(&theme, &SystemTheme::changed);
        writeAtomically(SystemTheme::runtimeDir() + "/system.json", snapshot("Night", "dark", "#000010", "#f0f0f0", "#33cc55"));
        QTRY_COMPARE_WITH_TIMEOUT(theme.themeName(), QStringLiteral("Night"), 5000);
        QVERIFY(!theme.light());
        QCOMPARE(theme.color("surface.primary"), QColor("#000010"));
    }

    void theSuiteGivesSizeDensityAndFilesAccent() {
        writeAtomically(SystemTheme::runtimeDir() + "/system.json", snapshot("Paper", "light", "#fafafa", "#202020", "#cc3355"));
        writeAtomically(SystemTheme::suitePath(),
                        "mode = \"theme\"\ntheme = \"paper\"\ndensity = \"compact\"\ntext_scale = 1.2 # larger\n\n[app_accents]\nfiles = \"green\"\nmail = \"amber\"\n");
        SystemTheme theme;
        QCOMPARE(theme.textScale(), 1.2);
        QCOMPARE(theme.density(), QStringLiteral("compact"));
        QCOMPARE(theme.accent(), QColor("#22aa44"));
        // Out-of-range sizes are held to Nyx's bounds.
        writeAtomically(SystemTheme::suitePath(), "text_scale = 3\n");
        QTRY_COMPARE_WITH_TIMEOUT(theme.textScale(), 1.25, 5000);
        QCOMPARE(theme.density(), QStringLiteral("comfortable"));
        QCOMPARE(theme.accent(), QColor("#cc3355"));
        QFile::remove(SystemTheme::suitePath());
        QFile::remove(SystemTheme::runtimeDir() + "/system.json");
    }

    void filesAccentIsKeptInItsOwnLine() {
        const QString suite = "# my settings\nmode = \"theme\"\n\n[app_accents]\nmail = \"amber\" # kept\n\n[tokens]\n\"accent.primary\" = \"#ff8800\"\n";
        // Added under the table that exists, the rest untouched.
        const QString added = SystemTheme::withFilesAccent(suite, "violet");
        QCOMPARE(added, QString("# my settings\nmode = \"theme\"\n\n[app_accents]\nfiles = \"violet\"\nmail = \"amber\" # kept\n\n[tokens]\n\"accent.primary\" = \"#ff8800\"\n"));
        // Changed in place, then taken out.
        QCOMPARE(SystemTheme::withFilesAccent(added, "green"), QString(added).replace("violet", "green"));
        QCOMPARE(SystemTheme::withFilesAccent(added, ""), suite);
        // A files key in another table is not ours.
        QCOMPARE(SystemTheme::withFilesAccent("[applications]\nfiles = 1\n", "teal"), QString("[applications]\nfiles = 1\n\n[app_accents]\nfiles = \"teal\"\n"));
        QCOMPARE(SystemTheme::withFilesAccent("", "teal"), QString("[app_accents]\nfiles = \"teal\"\n"));
        QCOMPARE(SystemTheme::withFilesAccent("mode = \"theme\"\n", ""), QString("mode = \"theme\"\n"));

        // Chosen in Files: written, and followed.
        writeAtomically(SystemTheme::runtimeDir() + "/system.json", snapshot("Paper", "light", "#fafafa", "#202020", "#cc3355"));
        writeAtomically(SystemTheme::suitePath(), suite.toUtf8());
        SystemTheme theme;
        theme.setAccentChoice("violet");
        QCOMPARE(theme.accentChoice(), QStringLiteral("violet"));
        QCOMPARE(theme.accent(), QColor("#7755cc"));
        QFile file(SystemTheme::suitePath());
        QVERIFY(file.open(QIODevice::ReadOnly));
        QCOMPARE(QString::fromUtf8(file.readAll()), added);
        theme.setAccentChoice("");
        QCOMPARE(theme.accent(), QColor("#cc3355"));
        QFile::remove(SystemTheme::suitePath());
        QFile::remove(SystemTheme::runtimeDir() + "/system.json");
    }
};

QTEST_MAIN(TestSystemTheme)
#include "tst_systemtheme.moc"
