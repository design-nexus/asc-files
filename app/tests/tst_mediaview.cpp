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

        // 10xer's keys, the preview holding them: volume, mute and seeking.
        const auto key = [&](int code, const QString &text = {}) {
            QVariant handled;
            const QVariant event = QVariantMap{{"key", code}, {"text", text}, {"modifiers", 0}};
            QMetaObject::invokeMethod(view.get(), "key", Q_RETURN_ARG(QVariant, handled), Q_ARG(QVariant, event));
            return handled.toBool();
        };
        AppSettings::shared().setMediaVolume(0.5);
        QVERIFY(key(Qt::Key_Up));
        QCOMPARE(qRound(AppSettings::shared().mediaVolume() * 10), 6);
        QVERIFY(key(Qt::Key_Minus, QStringLiteral("-")));
        QCOMPARE(qRound(AppSettings::shared().mediaVolume() * 10), 5);
        QVERIFY(key(Qt::Key_M, QStringLiteral("m")));
        auto *output = qvariant_cast<QObject *>(player->property("audioOutput"));
        QVERIFY(output && output->property("muted").toBool());
        QVERIFY(key(Qt::Key_End));
        QTRY_VERIFY(player->property("position").toLongLong() > 500);
        QVERIFY(key(Qt::Key_Home));
        QTRY_COMPARE(player->property("position").toLongLong(), 0);
        QVERIFY(!key(Qt::Key_X, QStringLiteral("x")));
    }
};

QTEST_MAIN(TestMediaView)
#include "tst_mediaview.moc"
