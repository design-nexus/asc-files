// SPDX-License-Identifier: MIT
#include "backend/Wire.h"

#include <QJsonObject>
#include <QTest>

class TestWire : public QObject {
    Q_OBJECT
private slots:
    void textStaysAString() {
        QCOMPARE(Wire::encode("café"), QJsonValue(QStringLiteral("café")));
        QCOMPARE(Wire::decode(QStringLiteral("café")), QByteArray("café"));
    }

    void bytesThatAreNotUtf8RoundTrip() {
        const QByteArray raw("caf\xe9.txt");
        const QJsonValue sent = Wire::encode(raw);
        QVERIFY(sent.isObject());
        QCOMPARE(sent.toObject().value("bytes").toString(), QStringLiteral("Y2Fm6S50eHQ="));
        QCOMPARE(Wire::display(sent), QStringLiteral("caf�.txt"));
        QCOMPARE(Wire::decode(sent), raw);
        // The backend's own form, without display text.
        QCOMPARE(Wire::decode(QJsonObject{{"bytes", "Y2Fm6S50eHQ="}}), raw);
        QCOMPARE(Wire::display(QJsonObject{{"bytes", "Y2Fm6S50eHQ="}}), QStringLiteral("caf�.txt"));
    }

    void malformedValuesDecodeToNothing() {
        QCOMPARE(Wire::decode(QJsonValue(3)), QByteArray());
        QCOMPARE(Wire::decode(QJsonObject{{"bytes", "!!"}}), QByteArray());
    }

    void pathsJoinAndSplit() {
        QCOMPARE(Wire::join("/a", "b"), QByteArray("/a/b"));
        QCOMPARE(Wire::join("/", "b"), QByteArray("/b"));
        QCOMPARE(Wire::parent("/a/b"), QByteArray("/a"));
        QCOMPARE(Wire::parent("/a/b/"), QByteArray("/a"));
        QCOMPARE(Wire::parent("/a"), QByteArray("/"));
        QCOMPARE(Wire::parent("/"), QByteArray("/"));
        QCOMPARE(Wire::baseName("/a/caf\xe9"), QByteArray("caf\xe9"));
    }
};

QTEST_GUILESS_MAIN(TestWire)
#include "tst_wire.moc"
