// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QList>
#include <QObject>
#include <QStringList>

class Windows;

/// One window to open for `org.freedesktop.FileManager1`: the folder to show, the items to
/// select in it (raw paths, so names that aren't UTF-8 stay distinct), and whether the caller
/// asked for their properties.
struct Reveal {
    QByteArray folder;
    QList<QByteArray> items;
    bool properties = false;
    bool operator==(const Reveal &other) const = default;
};

/// `org.freedesktop.FileManager1`, which browsers and other apps call for "Show in folder" (they
/// talk to that bus name directly, not through `xdg-open`), as legacy's.
class FileManager1 : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.FileManager1")
public:
    enum class Method { Folders, Items, ItemProperties };
    using QObject::QObject;

    /// The windows a call asks for: one per folder, in the order the URIs name them; items and
    /// item properties show their folder with them selected (a root, having none, opens itself).
    /// Only local (`file:`) URIs are shown.
    static QList<Reveal> revealRequests(Method method, const QStringList &uris);

public slots:
    Q_SCRIPTABLE void ShowFolders(const QStringList &uris, const QString &startupId);
    Q_SCRIPTABLE void ShowItems(const QStringList &uris, const QString &startupId);
    Q_SCRIPTABLE void ShowItemProperties(const QStringList &uris, const QString &startupId);

signals:
    void reveal(const Reveal &reveal);
};

/// Files on the session bus: one Files per session, which owns its own name (asked to open
/// folders by any later launch) and `org.freedesktop.FileManager1`.
namespace DesktopBus {

inline constexpr char ServiceName[] = "io.github.design_nexus.ascendant.Files";
inline constexpr char ObjectPath[] = "/io/github/design_nexus/ascendant/Files";

/// Claims Files' names for `windows`. False when another Files already runs: `folders` went to
/// it, and this one has nothing to do. True too where there is no session bus.
bool claim(Windows *windows, const QStringList &folders);

} // namespace DesktopBus
