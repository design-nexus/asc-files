// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "desktop/DesktopApps.h"

#include <QDir>
#include <QMimeDatabase>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>
#include <QTemporaryDir>

namespace {

void write(const QString &path, const QByteArray &contents) {
    QDir().mkpath(QFileInfo(path).path());
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(contents);
}

QStringList ids(const QVariantList &apps) {
    QStringList ids;
    for (const QVariant &app : apps) {
        ids.append(app.toMap().value("id").toString());
    }
    return ids;
}

} // namespace

class TestDesktopApps : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() { TestSupport::requirePrivateHome(); }

    void entriesAreReadAsTheSpecificationSays() {
        bool ok = false;
        const QByteArray viewer = "[Desktop Entry]\nType=Application\nName=Viewer\nName[de]=Betrachter\nName[de_AT]=Schauer\n"
                                  "Exec=viewer %f\nIcon=viewer\nMimeType=image/png;image/jpeg;\nComment=A\\sviewer\n"
                                  "[Desktop Action new]\nName=Other\nExec=other\n";
        DesktopApps::App app = DesktopApps::parseEntry("/x/viewer.desktop", "viewer.desktop", viewer, "de_AT", &ok);
        QVERIFY(ok);
        QCOMPARE(app.name, QStringLiteral("Schauer"));
        QCOMPARE(app.exec, QStringLiteral("viewer %f"));
        QCOMPARE(app.mimeTypes, (QStringList{"image/png", "image/jpeg"}));
        QCOMPARE(DesktopApps::parseEntry("", "v", viewer, "de_CH", &ok).name, QStringLiteral("Betrachter"));
        QCOMPARE(DesktopApps::parseEntry("", "v", viewer, "fr_FR", &ok).name, QStringLiteral("Viewer"));
        // Not programs to offer: hidden, a link, one whose TryExec isn't installed.
        DesktopApps::parseEntry("", "v", "[Desktop Entry]\nType=Application\nName=V\nExec=v\nHidden=true\n", "en", &ok);
        QVERIFY(!ok);
        DesktopApps::parseEntry("", "v", "[Desktop Entry]\nType=Link\nName=V\nURL=https://example.com\n", "en", &ok);
        QVERIFY(!ok);
        DesktopApps::parseEntry("", "v", "[Desktop Entry]\nType=Application\nName=V\nExec=v\nTryExec=asc-files-no-such-program\n", "en", &ok);
        QVERIFY(!ok);
    }

    void execIsQuotedAsTheSpecificationSays() {
        bool ok = false;
        const QString exec = QStringLiteral("sh -c \"echo \\\"a b\\\" \\$HOME \\\\ \\`x\\`\" %F");
        const QStringList expected{"sh", "-c", QStringLiteral("echo \"a b\" $HOME \\ `x`"), "%F"};
        QCOMPARE(DesktopApps::splitExec(exec, &ok), expected);
        QVERIFY(ok);
        const QString unclosed = QStringLiteral("sh -c \"unclosed");
        DesktopApps::splitExec(unclosed, &ok);
        QVERIFY(!ok);

        DesktopApps::App app;
        app.name = "Viewer";
        app.icon = "viewer";
        app.file = "/apps/viewer.desktop";
        const QStringList two{"/a/one.png", "/a/two words.png"};
        // One file at a time: started once for each.
        app.exec = "viewer --file=%f --name %c";
        QCOMPARE(DesktopApps::commandLines(app, two),
                 (QList<QStringList>{{"viewer", "--file=/a/one.png", "--name", "Viewer"}, {"viewer", "--file=/a/two words.png", "--name", "Viewer"}}));
        app.exec = "viewer %U %i %k 100%% %d";
        QCOMPARE(DesktopApps::commandLines(app, two),
                 (QList<QStringList>{{"viewer", "file:///a/one.png", "file:///a/two%20words.png", "--icon", "viewer", "/apps/viewer.desktop", "100%"}}));
        app.exec = "viewer %F";
        QCOMPARE(DesktopApps::commandLines(app, two), (QList<QStringList>{{"viewer", "/a/one.png", "/a/two words.png"}}));
    }

    void aDefaultIsWrittenWithoutTouchingTheRest() {
        const QString list = "# mine\n[Default Applications]\ntext/plain=old.desktop\n\n[Added Associations]\ntext/plain=a.desktop;new.desktop;\n\n[Other]\nkept=1\n";
        QCOMPARE(DesktopApps::withDefault(list, "new.desktop", {"text/plain", "image/png"}),
                 QString("# mine\n[Default Applications]\ntext/plain=new.desktop;\nimage/png=new.desktop;\n\n[Added Associations]\n"
                         "text/plain=new.desktop;a.desktop;\nimage/png=new.desktop;\n\n[Other]\nkept=1\n"));
        QCOMPARE(DesktopApps::withDefault("", "new.desktop", {"text/plain"}),
                 QString("[Default Applications]\ntext/plain=new.desktop;\n\n[Added Associations]\ntext/plain=new.desktop;\n"));
    }

    void programsAreOfferedStartedAndMadeDefault() {
        const QString apps = QStandardPaths::writableLocation(QStandardPaths::ApplicationsLocation);
        QVERIFY(apps.contains("asc-files-test-home"));
        QTemporaryDir dir;
        // A program that writes down what it was given.
        const QString script = dir.filePath("record.sh");
        write(script, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/given\"\n");
        QFile::setPermissions(script, QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner);
        const auto entry = [](const QString &name, const QString &exec, const QString &types, const QString &extra = {}) {
            return QStringLiteral("[Desktop Entry]\nType=Application\nName=%1\nExec=%2\nMimeType=%3\n%4").arg(name, exec, types, extra).toUtf8();
        };
        write(apps + "/asc-test-editor.desktop", entry("Asc Editor", "true %F", "text/plain;"));
        write(apps + "/asc-test-viewer.desktop", entry("Asc Viewer", script + " %F", "text/plain;"));
        write(apps + "/asc-test-gone.desktop", entry("Asc Gone", "true %F", "text/plain;"));
        write(apps + "/asc-test-hidden.desktop", entry("Asc Hidden", "true %F", "text/plain;", "Hidden=true\n"));
        write(apps + "/sub/asc-test-sub.desktop", entry("Asc C", "true %F", "text/x-csrc;"));
        write(QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + "/mimeapps.list",
              "[Default Applications]\ntext/plain=asc-test-editor.desktop\n\n[Removed Associations]\ntext/plain=asc-test-gone.desktop\n");
        write(dir.filePath("notes.txt"), "hello\n");
        write(dir.filePath("main.c"), "#include <stdio.h>\nint main(void) { return 0; }\n");

        DesktopApps desktop;
        const QVariantList forText = desktop.appsFor({dir.filePath("notes.txt")});
        QVERIFY(!forText.isEmpty());
        QCOMPARE(forText.first().toMap().value("id").toString(), QStringLiteral("asc-test-editor.desktop"));
        QVERIFY(forText.first().toMap().value("isDefault").toBool());
        QVERIFY(ids(forText).contains("asc-test-viewer.desktop"));
        QVERIFY(!ids(forText).contains("asc-test-gone.desktop"));
        QVERIFY(!ids(forText).contains("asc-test-hidden.desktop"));
        QVERIFY(!ids(forText).contains("sub-asc-test-sub.desktop"));
        QCOMPARE(desktop.appsFor({dir.filePath("notes.txt")}, 1).size(), 1);
        // A C file is a text file too; its own program is named by the folder it is in.
        const QStringList forC = ids(desktop.appsFor({dir.filePath("main.c")}));
        QVERIFY2(forC.contains("sub-asc-test-sub.desktop"), qPrintable(forC.join(", ")));
        QVERIFY(forC.contains("asc-test-viewer.desktop"));
        // Both: only what opens both.
        const QStringList forBoth = ids(desktop.appsFor({dir.filePath("notes.txt"), dir.filePath("main.c")}));
        QVERIFY(forBoth.contains("asc-test-viewer.desktop"));
        QVERIFY(!forBoth.contains("sub-asc-test-sub.desktop"));
        QVERIFY(ids(desktop.otherApps({dir.filePath("notes.txt")})).contains("sub-asc-test-sub.desktop"));
        QVERIFY(!ids(desktop.otherApps({dir.filePath("notes.txt")})).contains("asc-test-editor.desktop"));

        // Started on the files.
        QCOMPARE(desktop.launch("asc-test-viewer.desktop", {dir.filePath("notes.txt"), dir.filePath("main.c")}), QString());
        QTRY_VERIFY_WITH_TIMEOUT(QFileInfo(dir.filePath("given")).size() > 0, 5000);
        QFile given(dir.filePath("given"));
        QVERIFY(given.open(QIODevice::ReadOnly));
        QCOMPARE(QString::fromUtf8(given.readAll()), dir.filePath("notes.txt") + "\n" + dir.filePath("main.c") + "\n");
        QVERIFY(!desktop.launch("asc-test-nothing.desktop", {}).isEmpty());

        // Made the default for text.
        QVERIFY(desktop.setDefault("asc-test-viewer.desktop", {dir.filePath("notes.txt")}));
        const QVariantList after = desktop.appsFor({dir.filePath("notes.txt")});
        QCOMPARE(after.first().toMap().value("id").toString(), QStringLiteral("asc-test-viewer.desktop"));
        QVERIFY(ids(after).contains("asc-test-editor.desktop"));
        QCOMPARE(desktop.typesText({dir.filePath("notes.txt")}), QMimeDatabase().mimeTypeForName("text/plain").comment());
    }
};

QTEST_GUILESS_MAIN(TestDesktopApps)
#include "tst_desktopapps.moc"
