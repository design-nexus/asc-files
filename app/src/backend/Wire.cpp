// SPDX-License-Identifier: MIT
#include "Wire.h"

#include <QJsonObject>
#include <QStringDecoder>

namespace Wire {

namespace {

bool isUtf8(const QByteArray &raw) {
    QStringDecoder decoder(QStringDecoder::Utf8, QStringDecoder::Flag::Stateless);
    [[maybe_unused]] const QString text = decoder.decode(raw);
    return !decoder.hasError();
}

} // namespace

QJsonValue encode(const QByteArray &raw) {
    if (isUtf8(raw)) {
        return QString::fromUtf8(raw);
    }
    return QJsonObject{{QStringLiteral("bytes"), QString::fromLatin1(raw.toBase64())},
                       {QStringLiteral("display"), display(raw)}};
}

QByteArray decode(const QJsonValue &value) {
    if (value.isString()) {
        return value.toString().toUtf8();
    }
    const QJsonValue bytes = value.toObject().value(QStringLiteral("bytes"));
    if (!bytes.isString()) {
        return {};
    }
    const auto decoded = QByteArray::fromBase64Encoding(bytes.toString().toLatin1(),
                                                        QByteArray::AbortOnBase64DecodingErrors);
    return decoded ? *decoded : QByteArray();
}

QString display(const QJsonValue &value) {
    if (value.isString()) {
        return value.toString();
    }
    const QJsonObject object = value.toObject();
    if (object.contains(QStringLiteral("display"))) {
        return object.value(QStringLiteral("display")).toString();
    }
    return display(decode(value));
}

QString display(const QByteArray &raw) {
    // Invalid sequences decode as U+FFFD, as the backend's display text does.
    return QString::fromUtf8(raw);
}

QByteArray join(const QByteArray &folder, const QByteArray &name) {
    if (folder.endsWith('/')) {
        return folder + name;
    }
    return folder + '/' + name;
}

QByteArray parent(const QByteArray &path) {
    QByteArray trimmed = path;
    while (trimmed.size() > 1 && trimmed.endsWith('/')) {
        trimmed.chop(1);
    }
    const qsizetype slash = trimmed.lastIndexOf('/');
    if (slash <= 0) {
        return QByteArrayLiteral("/");
    }
    return trimmed.left(slash);
}

QByteArray baseName(const QByteArray &path) {
    QByteArray trimmed = path;
    while (trimmed.size() > 1 && trimmed.endsWith('/')) {
        trimmed.chop(1);
    }
    return trimmed.mid(trimmed.lastIndexOf('/') + 1);
}

} // namespace Wire
