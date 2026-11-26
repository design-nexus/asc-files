// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QList>
#include <QSize>
#include <QString>
#include <chrono>
#include <optional>

/// Where the chooser's window goes on Hyprland (legacy's `window_geometry`): a window rule centres
/// the chooser's own class, and the caller's window size, where it has exactly one window, sizes it.
/// Every exchange with the compositor is bounded in time and size, and optional.
namespace Portal::Placement {

/// The chooser's window class (Wayland app ID), its own so a rule matches only it.
inline constexpr char ChooserClass[] = "asc-files-chooser";
inline constexpr std::chrono::milliseconds QueryTimeout{100};
inline constexpr qint64 MaxReplyBytes = 1024 * 1024;
inline constexpr qint64 MaxCommandReplyBytes = 8192;

/// Hyprland's command socket for an instance, when `runtime` is absolute and `signature` plain.
std::optional<QByteArray> socketPath(const QByteArray &runtime, const QString &signature);
/// The running Hyprland's socket, if any.
std::optional<QByteArray> hyprlandSocket();

/// The rule as Hyprland's Lua config takes it, then the older keyword form (probe, then filter
/// and effect in one batch, so a config reload can't drop the match between them).
QList<QByteArray> centeringCommands();
/// Sends `command`, reads the reply up to `maxReply` bytes, all before `timeout` ends.
std::optional<QByteArray> exchange(const QByteArray &socket, const QByteArray &command, qint64 maxReply,
                                   std::chrono::milliseconds timeout);
/// Installs the centring rule; false where the compositor won't (placement stays its own).
bool installCenteringRule(const QByteArray &socket, std::chrono::milliseconds timeout);

/// The size of `appId`'s window in a `j/clients` reply, when it has exactly one, mapped and shown.
std::optional<QSize> applicationWindowSize(const QByteArray &reply, const QString &appId);
std::optional<QSize> querySize(const QByteArray &socket, const QString &appId, std::chrono::milliseconds timeout);
/// The caller's window size for a request from a Wayland window of an identified app; else none.
std::optional<QSize> parentSizeHint(const QString &appId, const QString &parentWindow);

} // namespace Portal::Placement
