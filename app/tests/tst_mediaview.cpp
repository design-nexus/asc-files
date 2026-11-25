// SPDX-License-Identifier: MIT
#include "TestSupport.h"
#include "settings/AppSettings.h"
#include "theme/SystemTheme.h"

#include <QFile>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQmlExtensionPlugin>
#include <QQuickItem>
#include <QQuickStyle>
#include <QTemporaryDir>
#include <QtEndian>

Q_IMPORT_QML_PLUGIN(AscFilesPlugin)

namespace {

/// A second of 8 kHz mono silence, as a WAV file.
QByteArray silence() {
    const quint32 samples = 8000;
    QByteArray wav("RIFF");
    const auto u32 = [](quint32 value) { return QByteArray(reinterpret_cast<const char *>(&(value = qToLittleEndian(value))), 4); };
    const auto u16 = [](quint16 value) { return QByteArray(reinterpret_cast<const char *>(&(value = qToLittleEndian(value))), 2); };
    wav += u32(36 + samples * 2) + "WAVEfmt " + u32(16) + u16(1) + u16(1) + u32(8000) + u32(16000) + u16(2) + u16(16);
    wav += "data" + u32(samples * 2) + QByteArray(samples * 2, '\0');
    return wav;
}

} // namespace

class TestMediaView : public QObject {
    Q_OBJECT
private slots:
    void initTestCase() {
        TestSupport::requirePrivateHome();
        QQuickStyle::setStyle(QStringLiteral("Basic"));
        SystemTheme::registerType();
        AppSettings::registerType();
    }

    void soundIsReadAndNotStartedUnasked() {
        QTemporaryDir dir;
        QFile file(dir.filePath("quiet one.wav"));
        QVERIFY(file.open(QIODevice::WriteOnly));
        file.write(silence());
        file.close();
        QQmlEngine engine;
        QQmlComponent component(&engine, QStringLiteral("AscFiles"), QStringLiteral("MediaView"));
        std::unique_ptr<QObject> view(component.createWithInitialProperties({{"path", file.fileName()}}));
        QVERIFY2(view, qPrintable(component.errorString()));
        auto *player = view->findChild<QObject *>(QStringLiteral("mediaPlayer"));
        QVERIFY(player);
        // Its length is read; it doesn't play, as Settings doesn't say to.
        QTRY_VERIFY_WITH_TIMEOUT(player->property("duration").toLongLong() > 900, 10000);
        QVERIFY(!player->property("playing").toBool());
        QCOMPARE(player->property("source").toUrl().toLocalFile(), file.fileName());
    }
};

QTEST_MAIN(TestMediaView)
#include "tst_mediaview.moc"
