// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "models/Customizations.h"

#include <QFile>
#include <QSignalSpy>

class TestCustomizations : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        QVERIFY(Customizations::filePath().contains("asc-files-test-home"));
    }

    void choicesAreKeptAndChecked() {
        Customizations &customizations = Customizations::shared();
        QCOMPARE(customizations.colors().size(), 7);
        QCOMPARE(customizations.icons().size(), 16);
        QSignalSpy changed(&customizations, &Customizations::changed);
        customizations.setColor({"/a/photos", "/a/music"}, "purple");
        QCOMPARE(changed.count(), 1);
        QCOMPARE(customizations.colorOf("/a/photos"), QStringLiteral("#8e4ec6"));
        QCOMPARE(customizations.chosenColor("/a/music"), QStringLiteral("purple"));
        customizations.setColor({"/a/photos"}, "#12AB34");
        QCOMPARE(customizations.colorOf("/a/photos"), QStringLiteral("#12ab34"));
        // Not a colour on offer nor #rrggbb; not an icon on offer nor an emoji: refused.
        customizations.setColor({"/a/photos"}, "chartreuse");
        customizations.setIcon({"/a/photos"}, "spaceship");
        QCOMPARE(changed.count(), 2);
        customizations.setIcon({"/a/photos"}, "pictures");
        QVERIFY(customizations.iconOf("/a/photos").startsWith('M'));
        customizations.setIcon({"/a/music"}, "emoji:🎵");
        QCOMPARE(customizations.iconOf("/a/music"), QStringLiteral("emoji:🎵"));
        // Kept in the file.
        QFile file(Customizations::filePath());
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QByteArray kept = file.readAll();
        QVERIFY(kept.contains("purple") && kept.contains("pictures"));
        customizations.reset({"/a/photos", "/a/music"});
        QCOMPARE(customizations.colorOf("/a/music"), QString());
        QCOMPARE(customizations.iconOf("/a/photos"), QString());
    }
};

QTEST_GUILESS_MAIN(TestCustomizations)
#include "tst_customizations.moc"
