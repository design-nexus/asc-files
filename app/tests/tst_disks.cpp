// SPDX-License-Identifier: MIT
//! Which of UDisks2's objects the sidebar shows, and as what.
#include "desktop/Disks.h"
#include "models/PlacesModel.h"

#include <QProcess>
#include <QSignalSpy>

#include <QTest>
#include <unistd.h>

namespace {

using Interfaces = QMap<QString, QVariantMap>;

QVariantMap block(const QByteArray &device, const QString &drive, qint64 size, const QString &label = {}, bool system = false,
                  bool ignore = false, const QString &backing = "/") {
    return {{"Device", QByteArray(device + '\0')},
            {"PreferredDevice", QByteArray(device + '\0')},
            {"Drive", QVariant::fromValue(QDBusObjectPath(drive))},
            {"Size", size},
            {"IdLabel", label},
            {"IdType", "vfat"},
            {"HintSystem", system},
            {"HintIgnore", ignore},
            {"CryptoBackingDevice", QVariant::fromValue(QDBusObjectPath(backing))}};
}

QVariantMap filesystem(const QByteArray &mountPoint = {}) {
    QList<QByteArray> points;
    if (!mountPoint.isEmpty()) {
        points.append(mountPoint + '\0');
    }
    return {{"MountPoints", QVariant::fromValue(points)}};
}

QVariantMap drive(bool removable, bool ejectable) { return {{"Removable", removable}, {"Ejectable", ejectable}, {"CanPowerOff", removable}}; }

const QString Root = QStringLiteral("/org/freedesktop/UDisks2/");

void add(ManagedObjects &objects, const QString &path, Interfaces interfaces) { objects.insert(QDBusObjectPath(Root + path), interfaces); }

} // namespace

class TestDisks : public QObject {
    Q_OBJECT
private slots:
    void theSidebarShowsTheUsersVolumesOnce() {
        ManagedObjects objects;
        add(objects, "drives/nvme", {{"org.freedesktop.UDisks2.Drive", drive(false, false)}});
        add(objects, "drives/usb", {{"org.freedesktop.UDisks2.Drive", drive(true, true)}});
        // The system's own disk and an ignored device: not shown.
        add(objects, "block_devices/nvme0n1p2",
            {{"org.freedesktop.UDisks2.Block", block("/dev/nvme0n1p2", Root + "drives/nvme", 500'000'000'000, "nyxos", true)},
             {"org.freedesktop.UDisks2.Filesystem", filesystem("/")}});
        add(objects, "block_devices/zram0",
            {{"org.freedesktop.UDisks2.Block", block("/dev/zram0", "/", 1000, {}, true, true)}, {"org.freedesktop.UDisks2.Filesystem", filesystem()}});
        // A partition table, nothing to mount: not shown.
        add(objects, "block_devices/sda", {{"org.freedesktop.UDisks2.Block", block("/dev/sda", Root + "drives/usb", 64'000'000'000)}});
        // A stick, mounted; an unlabelled volume, not; a locked one; an unlocked one (twice: the
        // encrypted block and the filesystem inside, shown once).
        add(objects, "block_devices/sda1",
            {{"org.freedesktop.UDisks2.Block", block("/dev/sda1", Root + "drives/usb", 16'000'000'000, "STICK")},
             {"org.freedesktop.UDisks2.Filesystem", filesystem("/run/media/u/STICK")}});
        add(objects, "block_devices/sda2",
            {{"org.freedesktop.UDisks2.Block", block("/dev/sda2", Root + "drives/usb", 8'000'000'000)}, {"org.freedesktop.UDisks2.Filesystem", filesystem()}});
        add(objects, "block_devices/sda3",
            {{"org.freedesktop.UDisks2.Block", block("/dev/sda3", Root + "drives/usb", 4'000'000'000)},
             {"org.freedesktop.UDisks2.Encrypted", QVariantMap{{"CleartextDevice", QVariant::fromValue(QDBusObjectPath("/"))}}}});
        add(objects, "block_devices/sda4",
            {{"org.freedesktop.UDisks2.Block", block("/dev/sda4", Root + "drives/usb", 2'000'000'000)},
             {"org.freedesktop.UDisks2.Encrypted", QVariantMap{{"CleartextDevice", QVariant::fromValue(QDBusObjectPath(Root + "block_devices/dm_0"))}}}});
        add(objects, "block_devices/dm_0",
            {{"org.freedesktop.UDisks2.Block", block("/dev/dm-0", "/", 2'000'000'000, "Secret", true, false, Root + "block_devices/sda4")},
             {"org.freedesktop.UDisks2.Filesystem", filesystem("/run/media/u/Secret")}});

        // Disk images: the user's own shows, another user's doesn't.
        add(objects, "block_devices/loop0",
            {{"org.freedesktop.UDisks2.Block", block("/dev/loop0", "/", 32'000'000, "IMAGE", true)},
             {"org.freedesktop.UDisks2.Filesystem", filesystem()},
             {"org.freedesktop.UDisks2.Loop", QVariantMap{{"SetupByUID", uint(::getuid())}}}});
        add(objects, "block_devices/loop1",
            {{"org.freedesktop.UDisks2.Block", block("/dev/loop1", "/", 32'000'000, "OTHER", true)},
             {"org.freedesktop.UDisks2.Filesystem", filesystem()},
             {"org.freedesktop.UDisks2.Loop", QVariantMap{{"SetupByUID", uint(::getuid() + 1)}}}});
        const QList<Volume> volumes = Disks::volumesFrom(objects);
        QStringList devices;
        for (const Volume &volume : volumes) {
            devices << volume.device;
        }
        QCOMPARE(devices, (QStringList{"/dev/dm-0", "/dev/loop0", "/dev/sda1", "/dev/sda2", "/dev/sda3"}));
        QCOMPARE(volumes[1].name, QStringLiteral("IMAGE"));
        QList<Volume> withoutImage = volumes;
        withoutImage.removeAt(1);
        const Volume &secret = withoutImage[0];
        QCOMPARE(secret.name, QStringLiteral("Secret"));
        QCOMPARE(secret.mountPoint, QStringLiteral("/run/media/u/Secret"));
        QCOMPARE(secret.encrypted, Root + "block_devices/sda4");
        QCOMPARE(secret.drive, Root + "drives/usb");
        QVERIFY(secret.ejectable && !secret.locked);
        QCOMPARE(withoutImage[1].name, QStringLiteral("STICK"));
        QCOMPARE(withoutImage[1].mountPoint, QStringLiteral("/run/media/u/STICK"));
        QVERIFY(withoutImage[2].name.endsWith("Volume"));
        QVERIFY(withoutImage[2].mountPoint.isEmpty());
        QVERIFY(withoutImage[3].locked);
        QVERIFY(withoutImage[3].name.endsWith("Encrypted Volume"));
    }

    // Legacy's drive_ops tests on labels.
    void filesystemLabelLimitsMatchBackends() {
        QCOMPARE(Disks::maxLabelLength("vfat"), 11);
        QCOMPARE(Disks::maxLabelLength("ntfs"), 32);
        QCOMPARE(Disks::maxLabelLength("exfat"), 32);
        QVERIFY(Disks::labelError("vfat", QString(11, u'A')).isEmpty());
        QVERIFY(!Disks::labelError("vfat", QString(12, u'A')).isEmpty());
        QVERIFY(!Disks::labelError("ext4", QString(17, u'A')).isEmpty());
    }

    void labelCharactersFollowTheFilesystemRules() {
        for (const QChar c : QStringLiteral("*?.,;:/\\|+=<>[]\"")) {
            QVERIFY2(!Disks::labelError("vfat", QStringLiteral("A%1B").arg(c)).isEmpty(), qPrintable(QString(c)));
        }
        for (const QChar c : QStringLiteral("*?:/\\|<>\"")) {
            QVERIFY2(!Disks::labelError("exfat", QStringLiteral("A%1B").arg(c)).isEmpty(), qPrintable(QString(c)));
        }
        for (const char *type : {"vfat", "exfat", "ntfs"}) {
            QVERIFY(Disks::labelError(type, "BACKUP").isEmpty());
            QVERIFY(!Disks::labelError(type, QStringLiteral("A\u0001B")).isEmpty());
            QVERIFY(!Disks::labelError(type, QString::fromUtf8("A\0B", 3)).isEmpty());
        }
        QVERIFY(Disks::labelError("exfat", "BACKUP.2026").isEmpty());
        QVERIFY(Disks::labelError("ntfs", "BACKUP.2026").isEmpty());
    }

    // By hand (ASC_FILES_LIVE_DISKS=1): what the real UDisks2 shows, and the sidebar's rows.
    void whatTheSystemShows() {
        if (qEnvironmentVariable("ASC_FILES_LIVE_DISKS") != QLatin1String("1")) {
            QSKIP("set ASC_FILES_LIVE_DISKS=1 to read the system's drives");
        }
        Disks disks;
        QSignalSpy changed(&disks, &Disks::changed);
        QVERIFY(changed.wait(5000));
        PlacesModel places;
        places.setDisks(&disks);
        const auto show = [&] {
            for (const Volume &volume : disks.volumes()) {
                qInfo().noquote() << "volume" << volume.device << volume.name << "at" << volume.mountPoint << (volume.locked ? "locked" : "")
                                  << (volume.ejectable || volume.canPowerOff ? "ejectable" : "");
            }
            for (int row = 0; row < places.rowCount(); ++row) {
                const QModelIndex at = places.index(row);
                if (at.data(PlacesModel::SectionRole).toInt() == 3) {
                    qInfo().noquote() << "row" << at.data(PlacesModel::NameRole).toString() << at.data(PlacesModel::PathRole).toString()
                                      << at.data(PlacesModel::FreeTextRole).toString() << at.data(PlacesModel::VolumeRole).toString();
                }
            }
        };
        show();
        const QString device = qEnvironmentVariable("ASC_FILES_LIVE_DEVICE");
        if (!device.isEmpty()) {
            for (const Volume &volume : disks.volumes()) {
                if (volume.device == device) {
                    QSignalSpy mounted(&disks, &Disks::mounted);
                    disks.mount(volume.object);
                    QVERIFY(mounted.wait(10000));
                    qInfo().noquote() << "mounted at" << mounted.first().at(1).toString();
                    QTest::qWait(1500);
                    places.refreshDrives();
                    show();
                    disks.unmount(volume.object);
                    QVERIFY(changed.wait(5000));
                    QTest::qWait(500);
                    places.refreshDrives();
                    show();
                }
            }
        }
    }
};

QTEST_GUILESS_MAIN(TestDisks)
#include "tst_disks.moc"
