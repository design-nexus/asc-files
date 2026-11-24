// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QJsonValue>
#include <QString>

/// How paths and names travel in protocol v2 (docs/PROTOCOL.md): a JSON string when the bytes are
/// UTF-8, `{"bytes": "<base64>", "display": "…"}` when they are not. The app keeps paths as their
/// raw bytes and shows the display text, so no name is lost on its way to the backend and back.
namespace Wire {

/// The wire form of raw path or name bytes.
QJsonValue encode(const QByteArray &raw);

/// The raw bytes of a wire path or name; empty when the value is neither form.
QByteArray decode(const QJsonValue &value);

/// The text to show for a wire path or name.
QString display(const QJsonValue &value);

/// The text to show for raw bytes: UTF-8 where valid, U+FFFD for the rest.
QString display(const QByteArray &raw);

/// `folder` joined with one `name`, as raw bytes.
QByteArray join(const QByteArray &folder, const QByteArray &name);

/// The folder above `path` ("/" for a top-level item and for "/" itself).
QByteArray parent(const QByteArray &path);

/// The last name in `path`.
QByteArray baseName(const QByteArray &path);

} // namespace Wire
