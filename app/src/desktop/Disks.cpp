// SPDX-License-Identifier: MIT
#include "Disks.h"

#include <QDBusConnection>
#include <QDBusMetaType>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QLocale>
#include <QStandardPaths>
#include <QTimer>
#include <unistd.h>

namespace {

const QString Service = QStringLiteral("org.freedesktop.UDisks2");
const QString Root = QStringLiteral("/org/freedesktop/UDisks2");
const QString BlockInterface = QStringLiteral("org.freedesktop.UDisks2.Block");
const QString FilesystemInterface = QStringLiteral("org.freedesktop.UDisks2.Filesystem");
const QString EncryptedInterface = QStringLiteral("org.freedesktop.UDisks2.Encrypted");
const QString DriveInterface = QStringLiteral("org.freedesktop.UDisks2.Drive");

/// A path the bus sends as `ay`, without its trailing NUL.
QString bytesText(const QVariant &value) {
    QByteArray bytes = qdbus_cast<QByteArray>(value);
    if (bytes.endsWith('\0')) {
        bytes.chop(1);
    }
    return QString::fromLocal8Bit(bytes);
}

QString objectPath(const QVariant &value) {
    const QString path = qdbus_cast<QDBusObjectPath>(value).path();
    return path == QLatin1String("/") ? QString() : path;
}

QString firstMountPoint(const QVariantMap &filesystem) {
    const QList<QByteArray> points = qdbus_cast<QList<QByteArray>>(filesystem.value(QStringLiteral("MountPoints")));
    return points.isEmpty() ? QString() : bytesText(points.first());
}

} // namespace

QList<Volume> Disks::volumesFrom(const ManagedObjects &objects) {
    const auto interfaces = [&](const QString &object) { return objects.value(QDBusObjectPath(object)); };
    QList<Volume> volumes;
    for (auto it = objects.constBegin(); it != objects.constEnd(); ++it) {
        const QMap<QString, QVariantMap> &here = it.value();
        const bool filesystem = here.contains(FilesystemInterface);
        const bool encrypted = here.contains(EncryptedInterface);
        if (!here.contains(BlockInterface) || (!filesystem && !encrypted)) {
            continue;
        }
        const QVariantMap block = here.value(BlockInterface);
        Volume volume;
        volume.object = it.key().path();
        volume.device = bytesText(block.value(QStringLiteral("PreferredDevice")));
        if (volume.device.isEmpty()) {
            volume.device = bytesText(block.value(QStringLiteral("Device")));
        }
        volume.label = block.value(QStringLiteral("IdLabel")).toString();
        volume.type = block.value(QStringLiteral("IdType")).toString();
        volume.size = block.value(QStringLiteral("Size")).toLongLong();
        // An unlocked volume is shown as the filesystem inside it; the drive and hints are the
        // encrypted block's.
        QVariantMap hints = block;
        volume.encrypted = objectPath(block.value(QStringLiteral("CryptoBackingDevice")));
        if (!volume.encrypted.isEmpty()) {
            hints = interfaces(volume.encrypted).value(BlockInterface);
        }
        if (encrypted) {
            const QString cleartext = objectPath(here.value(EncryptedInterface).value(QStringLiteral("CleartextDevice")));
            if (!cleartext.isEmpty() && objects.contains(QDBusObjectPath(cleartext))) {
                continue;
            }
            volume.locked = !filesystem;
        }
        volume.drive = objectPath(hints.value(QStringLiteral("Drive")));
        const QVariantMap drive = interfaces(volume.drive).value(DriveInterface);
        volume.removable = drive.value(QStringLiteral("Removable")).toBool() || drive.value(QStringLiteral("MediaRemovable")).toBool();
        volume.ejectable = drive.value(QStringLiteral("Ejectable")).toBool();
        volume.canPowerOff = drive.value(QStringLiteral("CanPowerOff")).toBool();
        // A disk image the user attached is theirs, as a removable drive is.
        const QString backingObject = volume.encrypted.isEmpty() ? it.key().path() : volume.encrypted;
        const QVariantMap loop = interfaces(backingObject).value(QStringLiteral("org.freedesktop.UDisks2.Loop"));
        const bool usersImage = !loop.isEmpty() && loop.value(QStringLiteral("SetupByUID")).toUInt() == ::getuid();
        if (hints.value(QStringLiteral("HintIgnore")).toBool() ||
            (hints.value(QStringLiteral("HintSystem")).toBool() && !volume.removable && !usersImage)) {
            continue;
        }
        if (filesystem) {
            volume.mountPoint = firstMountPoint(here.value(FilesystemInterface));
        }
        const QString size = QLocale().formattedDataSize(volume.size, 0, QLocale::DataSizeSIFormat);
        volume.name = !volume.label.isEmpty() ? volume.label
                      : volume.locked        ? QObject::tr("%1 Encrypted Volume").arg(size)
                                             : QObject::tr("%1 Volume").arg(size);
        volumes.append(volume);
    }
    std::sort(volumes.begin(), volumes.end(), [](const Volume &a, const Volume &b) { return a.device < b.device; });
    return volumes;
}

Disks::Disks(QObject *parent) : QObject(parent) {
    qDBusRegisterMetaType<QList<QByteArray>>();
    qDBusRegisterMetaType<QMap<QString, QVariantMap>>();
    qDBusRegisterMetaType<ManagedObjects>();
    QDBusConnection bus = QDBusConnection::systemBus();
    if (!bus.isConnected()) {
        return;
    }
    // Changes come in bursts (a drive plugged in adds several objects): read once they settle.
    auto *settle = new QTimer(this);
    settle->setSingleShot(true);
    settle->setInterval(200);
    connect(settle, &QTimer::timeout, this, &Disks::refresh);
    for (const auto &[interface, signal] : {std::pair{QStringLiteral("org.freedesktop.DBus.ObjectManager"), QStringLiteral("InterfacesAdded")},
                                            std::pair{QStringLiteral("org.freedesktop.DBus.ObjectManager"), QStringLiteral("InterfacesRemoved")}}) {
        bus.connect(Service, Root, interface, signal, settle, SLOT(start()));
    }
    // Mounting changes a filesystem's MountPoints, which comes as PropertiesChanged on its object.
    bus.connect(Service, QString(), QStringLiteral("org.freedesktop.DBus.Properties"), QStringLiteral("PropertiesChanged"), settle,
                SLOT(start()));
    refresh();
}

void Disks::refresh() {
    QDBusMessage call = QDBusMessage::createMethodCall(Service, Root, QStringLiteral("org.freedesktop.DBus.ObjectManager"),
                                                       QStringLiteral("GetManagedObjects"));
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(call), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *watcher) {
        watcher->deleteLater();
        const QDBusPendingReply<ManagedObjects> reply = *watcher;
        if (reply.isError()) {
            return;
        }
        const QList<Volume> volumes = volumesFrom(reply.value());
        if (volumes != m_volumes) {
            m_volumes = volumes;
            emit changed();
        }
    });
}

Volume Disks::volumeAt(const QString &path) const {
    for (const Volume &volume : m_volumes) {
        if (!volume.mountPoint.isEmpty() && volume.mountPoint == path) {
            return volume;
        }
    }
    return {};
}

Volume Disks::volume(const QString &object) const {
    for (const Volume &volume : m_volumes) {
        if (volume.object == object) {
            return volume;
        }
    }
    return {};
}

void Disks::call(const QString &object, const QString &interface, const QString &method, const QVariantList &arguments,
                 std::function<void(const QDBusMessage &)> done) {
    QDBusMessage message = QDBusMessage::createMethodCall(Service, object, interface, method);
    message.setArguments(arguments);
    // Polkit may ask the user; that takes as long as they take.
    message.setInteractiveAuthorizationAllowed(true);
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(message, 5 * 60 * 1000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, done](QDBusPendingCallWatcher *watcher) {
        watcher->deleteLater();
        const QDBusMessage reply = watcher->reply();
        if (reply.type() == QDBusMessage::ErrorMessage) {
            // Dismissing polkit's prompt is not a failure to report.
            if (!reply.errorName().endsWith(QLatin1String("NotAuthorizedDismissed"))) {
                emit failed(reply.errorMessage());
            }
            return;
        }
        done(reply);
        refresh();
    });
}

void Disks::mount(const QString &object) {
    call(object, FilesystemInterface, QStringLiteral("Mount"), {QVariantMap{}}, [this, object](const QDBusMessage &reply) {
        emit mounted(object, reply.arguments().value(0).toString());
    });
}

void Disks::unmount(const QString &object) {
    call(object, FilesystemInterface, QStringLiteral("Unmount"), {QVariantMap{}}, [](const QDBusMessage &) {});
}

void Disks::eject(const QString &object) {
    const Volume chosen = volume(object);
    if (chosen.drive.isEmpty()) {
        return;
    }
    // Everything on the drive comes off first, one step after another.
    QStringList steps;
    for (const Volume &volume : m_volumes) {
        if (volume.drive == chosen.drive) {
            if (!volume.mountPoint.isEmpty()) {
                steps << QStringLiteral("unmount ") + volume.object;
            }
            if (!volume.encrypted.isEmpty()) {
                steps << QStringLiteral("lock ") + volume.encrypted;
            }
        }
    }
    if (chosen.ejectable) {
        steps << QStringLiteral("eject ") + chosen.drive;
    } else if (chosen.canPowerOff) {
        steps << QStringLiteral("poweroff ") + chosen.drive;
    }
    auto next = std::make_shared<std::function<void()>>();
    *next = [this, steps, next, name = chosen.name]() mutable {
        if (steps.isEmpty()) {
            emit ejected(name);
            return;
        }
        const QString step = steps.takeFirst();
        const QString what = step.section(u' ', 0, 0);
        const QString target = step.section(u' ', 1);
        const QString interface = what == QLatin1String("unmount") ? FilesystemInterface
                                  : what == QLatin1String("lock")  ? EncryptedInterface
                                                                   : DriveInterface;
        const QString method = what == QLatin1String("unmount") ? QStringLiteral("Unmount")
                               : what == QLatin1String("lock")  ? QStringLiteral("Lock")
                               : what == QLatin1String("eject") ? QStringLiteral("Eject")
                                                                : QStringLiteral("PowerOff");
        call(target, interface, method, {QVariantMap{}}, [next](const QDBusMessage &) { (*next)(); });
    };
    (*next)();
}

void Disks::unlock(const QString &object, const QString &passphrase) {
    QDBusMessage message = QDBusMessage::createMethodCall(Service, object, EncryptedInterface, QStringLiteral("Unlock"));
    message.setArguments({passphrase, QVariantMap{}});
    message.setInteractiveAuthorizationAllowed(true);
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::systemBus().asyncCall(message, 5 * 60 * 1000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, object](QDBusPendingCallWatcher *watcher) {
        watcher->deleteLater();
        const QDBusMessage reply = watcher->reply();
        if (reply.type() == QDBusMessage::ErrorMessage) {
            if (reply.errorMessage().contains(QLatin1String("passphrase"), Qt::CaseInsensitive) ||
                reply.errorMessage().contains(QLatin1String("No key available"), Qt::CaseInsensitive)) {
                emit passphraseWrong(object);
            } else if (!reply.errorName().endsWith(QLatin1String("NotAuthorizedDismissed"))) {
                emit failed(reply.errorMessage());
            }
            return;
        }
        // Unlocked: what is inside mounts at once.
        mount(qdbus_cast<QDBusObjectPath>(reply.arguments().value(0)).path());
    });
}

void Disks::rename(const QString &object, const QString &label) {
    if (const QString why = labelError(volume(object).type, label); !why.isEmpty()) {
        emit failed(why);
        return;
    }
    call(object, FilesystemInterface, QStringLiteral("SetLabel"), {label, QVariantMap{}}, [](const QDBusMessage &) {});
}

int Disks::maxLabelLength(const QString &type) {
    if (type == QLatin1String("vfat")) {
        return 11;
    }
    if (type == QLatin1String("ext2") || type == QLatin1String("ext3") || type == QLatin1String("ext4")) {
        return 16;
    }
    return 32;
}

QString Disks::labelError(const QString &type, const QString &label) {
    // Counted in bytes where the filesystem stores bytes (FAT's 11), in characters otherwise.
    const qsizetype length = type == QLatin1String("vfat") || type.startsWith(QLatin1String("ext")) ? label.toUtf8().size() : label.size();
    if (length > maxLabelLength(type)) {
        return tr("Filesystem label is too long.");
    }
    const QString forbidden = type == QLatin1String("vfat")    ? QStringLiteral("*?.,;:/\\|+=<>[]\"")
                              : type == QLatin1String("exfat") ? QStringLiteral("*?:/\\|<>\"")
                                                               : QString();
    for (const QChar c : label) {
        if (c.unicode() < 0x20 || c.unicode() == 0x7f) {
            return tr("Labels cannot contain control characters.");
        }
        if (forbidden.contains(c)) {
            const QString name = type == QLatin1String("vfat") ? QStringLiteral("FAT32") : QStringLiteral("exFAT");
            return tr("%1 labels cannot contain “%2”.").arg(name, c);
        }
    }
    return {};
}

QStringList Disks::formattable() {
    QStringList types;
    for (const auto &[type, tools] : {std::pair{QStringLiteral("vfat"), QStringList{"mkfs.fat", "mkfs.vfat"}},
                                      std::pair{QStringLiteral("exfat"), QStringList{"mkfs.exfat"}},
                                      std::pair{QStringLiteral("ntfs"), QStringList{"mkfs.ntfs", "mkntfs"}}}) {
        if (std::any_of(tools.begin(), tools.end(), [](const QString &tool) { return !QStandardPaths::findExecutable(tool).isEmpty(); })) {
            types.append(type);
        }
    }
    return types;
}

void Disks::format(const QString &object, const QString &type, const QString &label, bool quick) {
    const Volume chosen = volume(object);
    if (chosen.object.isEmpty() || !chosen.removable || !formattable().contains(type)) {
        emit failed(tr("Only a removable drive can be formatted here."));
        return;
    }
    if (const QString why = labelError(type, label); !why.isEmpty()) {
        emit failed(why);
        return;
    }
    QVariantMap options{{"update-partition-type", true}};
    if (!label.isEmpty()) {
        options.insert(QStringLiteral("label"), label);
    }
    if (!quick) {
        options.insert(QStringLiteral("erase"), QStringLiteral("zero"));
    }
    if (type == QLatin1String("vfat")) {
        options.insert(QStringLiteral("mkfs-args"), QStringList{QStringLiteral("-F"), QStringLiteral("32")});
    }
    // The block to format: the encrypted one an unlocked volume is in, so the whole is erased.
    const QString block = chosen.encrypted.isEmpty() ? chosen.object : chosen.encrypted;
    const auto erase = [this, block, type, options] {
        call(block, BlockInterface, QStringLiteral("Format"), {type, options}, [](const QDBusMessage &) {});
    };
    if (chosen.mountPoint.isEmpty()) {
        erase();
    } else {
        call(chosen.object, FilesystemInterface, QStringLiteral("Unmount"), {QVariantMap{}}, [erase](const QDBusMessage &) { erase(); });
    }
}
