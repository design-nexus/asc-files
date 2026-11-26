// SPDX-License-Identifier: MIT
#pragma once

#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QList>
#include <QMap>
#include <QObject>
#include <QVariantMap>
#include <functional>

/// UDisks2's objects: by object path, each interface's properties.
using ManagedObjects = QMap<QDBusObjectPath, QMap<QString, QVariantMap>>;

/// A volume the user can use: a filesystem (mounted or not) or a locked encrypted one.
struct Volume {
    /// Its block object (for an unlocked encrypted volume, the cleartext one inside).
    QString object;
    QString device;
    /// What it is called: its label, else its size and kind ("16 GB Volume").
    QString name;
    QString label;
    QString mountPoint;
    /// Encrypted and locked: it unlocks before it mounts.
    bool locked = false;
    /// The encrypted block an unlocked volume is inside (it locks again on eject).
    QString encrypted;
    /// The drive it is on, and whether that drive can be ejected or powered off.
    QString drive;
    bool ejectable = false;
    bool canPowerOff = false;
    bool removable = false;
    qint64 size = 0;
    /// Its filesystem ("vfat", "ext4"), for what renaming allows.
    QString type;
    bool operator==(const Volume &) const = default;
};

/// The drives UDisks2 knows, for the sidebar's Storage: mounting, unmounting, ejecting, unlocking
/// and renaming them. Polkit asks for permission where the system wants it.
class Disks : public QObject {
    Q_OBJECT
public:
    explicit Disks(QObject *parent = nullptr);

    /// The volumes a user sees in `objects`: filesystems and encrypted volumes that aren't the
    /// system's own or hidden (a removable drive's always show), each once (an unlocked encrypted
    /// volume as the filesystem inside it), in device order.
    static QList<Volume> volumesFrom(const ManagedObjects &objects);

    QList<Volume> volumes() const { return m_volumes; }
    /// The volume mounted at `path`, if any.
    Volume volumeAt(const QString &path) const;
    Volume volume(const QString &object) const;

    Q_INVOKABLE void mount(const QString &object);
    Q_INVOKABLE void unmount(const QString &object);
    /// Unmounts what is on the volume's drive, locks an encrypted volume, then ejects (or powers
    /// off) the drive.
    Q_INVOKABLE void eject(const QString &object);
    Q_INVOKABLE void unlock(const QString &object, const QString &passphrase);
    Q_INVOKABLE void rename(const QString &object, const QString &label);
    /// Erases the volume as `type` (`vfat`, `exfat` or `ntfs`) named `label`, unmounting it first;
    /// `quick` skips zeroing it. Only a removable drive's volume.
    Q_INVOKABLE void format(const QString &object, const QString &type, const QString &label, bool quick);

    /// Why `label` can't name a `type` filesystem (too long, a character it refuses), or empty.
    Q_INVOKABLE static QString labelError(const QString &type, const QString &label);
    static int maxLabelLength(const QString &type);
    /// The filesystems this system can format (their tools are installed).
    Q_INVOKABLE static QStringList formattable();

signals:
    void changed();
    /// A volume was mounted at `path` (after a mount asked here): the window goes there.
    void mounted(const QString &object, const QString &path);
    /// A drive can be unplugged.
    void ejected(const QString &name);
    /// Unlocking asked for a passphrase that was wrong.
    void passphraseWrong(const QString &object);
    void failed(const QString &message);

private:
    void refresh();
    void call(const QString &object, const QString &interface, const QString &method, const QVariantList &arguments,
              std::function<void(const QDBusMessage &)> done);
    QList<Volume> m_volumes;
};
