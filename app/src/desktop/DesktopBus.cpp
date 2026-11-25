// SPDX-License-Identifier: MIT
#include "DesktopBus.h"

#include "../app/Windows.h"

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QFile>
#include <QFileInfo>
#include <QUrl>

namespace {

/// A `file:` URI's path, its bytes as they are (percent escapes decoded, never through UTF-8).
QByteArray localPath(const QString &uri) {
    const QUrl url(uri, QUrl::StrictMode);
    if (!url.isValid() || url.scheme() != QLatin1String("file") || (!url.host().isEmpty() && url.host() != QLatin1String("localhost"))) {
        return {};
    }
    QByteArray path = QByteArray::fromPercentEncoding(url.path(QUrl::FullyEncoded).toLatin1());
    while (path.size() > 1 && path.endsWith('/')) {
        path.chop(1);
    }
    return path.startsWith('/') ? path : QByteArray();
}

/// Files' own object: a later launch hands its folders to the Files already running.
class Launcher : public QObject {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "io.github.design_nexus.ascendant.Files")
public:
    explicit Launcher(Windows *windows) : QObject(windows), m_windows(windows) {}
public slots:
    Q_SCRIPTABLE void Open(const QStringList &folders) { m_windows->open(folders); }

private:
    Windows *m_windows;
};

} // namespace

QList<Reveal> FileManager1::revealRequests(Method method, const QStringList &uris) {
    QList<Reveal> reveals;
    for (const QString &uri : uris) {
        const QByteArray path = localPath(uri);
        if (path.isEmpty()) {
            continue;
        }
        QByteArray folder = path;
        QByteArray item;
        if (method != Method::Folders && path != "/") {
            const qsizetype slash = path.lastIndexOf('/');
            folder = slash == 0 ? QByteArray("/") : path.left(slash);
            item = path;
        }
        auto found = std::find_if(reveals.begin(), reveals.end(), [&](const Reveal &reveal) { return reveal.folder == folder; });
        if (found == reveals.end()) {
            reveals.append({folder, {}, method == Method::ItemProperties});
            found = reveals.end() - 1;
        }
        if (!item.isEmpty()) {
            found->items.append(item);
        }
    }
    return reveals;
}

void FileManager1::ShowFolders(const QStringList &uris, const QString &) {
    for (const Reveal &one : revealRequests(Method::Folders, uris)) {
        emit reveal(one);
    }
}

void FileManager1::ShowItems(const QStringList &uris, const QString &) {
    for (const Reveal &one : revealRequests(Method::Items, uris)) {
        emit reveal(one);
    }
}

void FileManager1::ShowItemProperties(const QStringList &uris, const QString &) {
    for (const Reveal &one : revealRequests(Method::ItemProperties, uris)) {
        emit reveal(one);
    }
}

bool DesktopBus::claim(Windows *windows, const QStringList &folders) {
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        return true;
    }
    if (!bus.registerService(QString::fromLatin1(ServiceName))) {
        // Another Files runs: it opens the folders.
        QStringList absolute;
        for (const QString &folder : folders) {
            absolute.append(QFileInfo(folder).absoluteFilePath());
        }
        QDBusMessage call = QDBusMessage::createMethodCall(QString::fromLatin1(ServiceName), QString::fromLatin1(ObjectPath),
                                                           QString::fromLatin1(ServiceName), QStringLiteral("Open"));
        call << absolute;
        const QDBusMessage answer = bus.call(call, QDBus::Block, 5000);
        return answer.type() == QDBusMessage::ErrorMessage;
    }
    bus.registerObject(QString::fromLatin1(ObjectPath), new Launcher(windows), QDBusConnection::ExportScriptableSlots);
    auto *fileManager = new FileManager1(windows);
    QObject::connect(fileManager, &FileManager1::reveal, windows,
                     [windows](const Reveal &one) { windows->reveal(one.folder, one.items, one.properties); });
    bus.registerObject(QStringLiteral("/org/freedesktop/FileManager1"), fileManager, QDBusConnection::ExportScriptableSlots);
    // Another file manager may hold it; then it answers instead.
    bus.registerService(QStringLiteral("org.freedesktop.FileManager1"));
    return true;
}

#include "DesktopBus.moc"
