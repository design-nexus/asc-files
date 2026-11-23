// SPDX-License-Identifier: MIT
//! What the drive under a path is, as far as jobs need to know: which drive, so writes to one
//! drive queue behind each other, and whether it is removable, so a batch written to it is synced
//! before it is called done.

use std::path::Path;

/// The device `path` is on, or None if it cannot be reached.
pub fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|meta| meta.dev())
}

/// Whether `path` is on a removable drive: USB, an SD card, or a disk the kernel marks
/// removable. A network share or a drive the kernel cannot place is not.
pub fn is_removable(path: &Path) -> bool {
    let Ok(stat) = rustix::fs::statx(
        rustix::fs::CWD,
        path,
        rustix::fs::AtFlags::empty(),
        rustix::fs::StatxFlags::BASIC_STATS,
    ) else {
        return false;
    };
    let node = format!(
        "/sys/dev/block/{}:{}",
        stat.stx_dev_major, stat.stx_dev_minor
    );
    let Ok(device) = std::fs::canonicalize(node) else {
        return false;
    };
    removable_in_sysfs(&device)
}

/// A block device's sysfs folder: a partition's flag is on its disk, the folder above it.
fn removable_in_sysfs(device: &Path) -> bool {
    let on_bus = device.components().any(|part| {
        part.as_os_str()
            .to_str()
            .is_some_and(|part| part.starts_with("usb") || part.starts_with("mmc"))
    });
    let flagged = [Some(device), device.parent()]
        .into_iter()
        .flatten()
        .any(|folder| {
            std::fs::read_to_string(folder.join("removable")).is_ok_and(|flag| flag.trim() == "1")
        });
    on_bus || flagged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usb_partition_or_a_flagged_disk_is_removable_and_an_internal_one_is_not() {
        let sysfs = tempfile::tempdir().unwrap();
        let usb = sysfs
            .path()
            .join("devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1:1.0/host0/block/sdb/sdb1");
        let nvme = sysfs
            .path()
            .join("devices/pci0000:00/0000:00:01.1/nvme/nvme0/nvme0n1/nvme0n1p2");
        let flagged = sysfs.path().join("devices/virtual/block/sdc/sdc1");
        for folder in [&usb, &nvme, &flagged] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(nvme.parent().unwrap().join("removable"), "0\n").unwrap();
        std::fs::write(flagged.parent().unwrap().join("removable"), "1\n").unwrap();

        assert!(removable_in_sysfs(&usb));
        assert!(!removable_in_sysfs(&nvme));
        assert!(removable_in_sysfs(&flagged));
    }
}
