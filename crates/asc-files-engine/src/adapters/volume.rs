// SPDX-License-Identifier: MIT

mod mount_capabilities;
pub(crate) mod mount_watch;
mod mounts;
mod names;
#[cfg(test)]
mod tests;

use std::time::Duration;

#[cfg(test)]
pub(crate) use mount_capabilities::forget_test_mount_capabilities;
pub(crate) use mount_capabilities::{
    MountCapabilities, MountKey, XattrNamespace, learn_mount_capability, mount_capabilities,
    teach_mount_capability,
};
#[cfg(test)]
pub(crate) use mounts::FAT32_MAX_FILE_SIZE;
#[cfg(test)]
pub(crate) use mounts::set_mountinfo_for_isolated_test;
pub(crate) use mounts::{
    CoarseTimes, Family, FuseFormat, LONGEST_NAME_NTFS_3G_RENAMES_OVER, MountKey as TrashMountKey,
    MountSite, MountTable, MountType, SillyNames, family, holds_symlinks, is_fuse_fs_type,
    max_file_size,
};
pub(crate) use names::{
    MountNames, NameEncoding, NameLimit, NameProfile, NameRules, PrivateUse, Reserved, StrictRule,
    Strictness, Strip, gvfs_bridge_profile, gvfs_profile,
};

pub(crate) const REMOTE_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
