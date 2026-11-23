// SPDX-License-Identifier: MIT

//! What the mount behind a folder says about itself: learned once per mount from mountinfo,
//! udev and `statfs`, and read once per operation.

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    ffi::OsStr,
    hash::{DefaultHasher, Hash, Hasher},
    os::fd::{AsFd, BorrowedFd},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use super::{
    fs_ops::{FsOps, fs},
    io_error,
    native_fs::{open_local_parent_directory, opened_path, os_error_detail},
};
use crate::{
    adapters::volume::{
        Family, FuseFormat, MountCapabilities, MountKey, MountNames, MountType, NameEncoding,
        NameLimit, NameProfile, Strictness, family, gvfs_bridge_profile, gvfs_profile,
        holds_symlinks, learn_mount_capability, max_file_size, mount_capabilities,
        teach_mount_capability,
    },
    services::{IdentityTraits, InodeNumbers, NameMode},
};

pub(super) fn known_mount(fs: &dyn FsOps, dir: BorrowedFd<'_>) -> (MountKey, MountCapabilities) {
    let key = fs.mount_key(dir);
    (key, known(fs, dir, key))
}

pub(super) fn known(fs: &dyn FsOps, dir: BorrowedFd<'_>, key: MountKey) -> MountCapabilities {
    let capabilities = mount_capabilities(key);
    if capabilities.family.is_some() {
        return capabilities;
    }
    learn_mount_type(fs, dir, key)
}

pub(super) fn identity_now(
    fs: &dyn FsOps,
    dir: BorrowedFd<'_>,
    family: Family,
    identity: IdentityTraits,
) -> IdentityTraits {
    if family == Family::Smb
        && fs
            .current_mount_type(dir)
            .is_some_and(|mount| mount.server_numbers())
    {
        return IdentityTraits {
            numbers: InodeNumbers::Kept,
            ..identity
        };
    }
    identity
}

pub(super) fn identity_at(fs: &dyn FsOps, dir: BorrowedFd<'_>) -> IdentityTraits {
    let (_, capabilities) = known_mount(fs, dir);
    let family = capabilities.family.unwrap_or(Family::Other);
    identity_now(fs, dir, family, capabilities.identity)
}

pub(super) fn with_links(
    family: Option<Family>,
    identity: IdentityTraits,
    links_keep_numbers: bool,
) -> IdentityTraits {
    if links_keep_numbers
        && family == Some(Family::Fuse)
        && identity.numbers == InodeNumbers::Renewed
    {
        return IdentityTraits {
            numbers: InodeNumbers::Kept,
            ..identity
        };
    }
    identity
}

/// For `fuseblk`, udev's record of the device tells which daemon serves it. What another
/// thread learned of the mount meanwhile stays.
fn learn_mount_type(fs: &dyn FsOps, dir: BorrowedFd<'_>, key: MountKey) -> MountCapabilities {
    let mount = fs.mount_type(dir);
    let mount_type = mount.as_ref().map(|(mount_type, _)| mount_type);
    let family = mount_type.map_or(Family::Other, |mount| family(&mount.fs_type));
    let identity = mount_type.map_or_else(IdentityTraits::default, MountType::identity);
    let device_format = mount_type
        .filter(|mount| mount.fs_type == "fuseblk")
        .and_then(|_| fs.device_format(dir).ok().flatten());
    let format = device_format.as_deref();
    let fuse_format = format.and_then(FuseFormat::of);
    let identity = IdentityTraits {
        folder_births: identity.folder_births && fuse_format != Some(FuseFormat::Exfat),
        ..identity
    };
    let coarse_times = mount_type.and_then(|mount| mount.coarse_times(fuse_format));
    let deferred_close = mount_type.and_then(MountType::deferred_close);
    let silly_names = mount_type.and_then(MountType::silly_names);
    let attribute_cache = mount_type.and_then(MountType::attribute_cache);
    let cuts_long_names_renamed_over =
        mount_type.is_some_and(|mount| mount.cuts_long_names_renamed_over(format));
    let names = match &mount {
        Some((mount_type, site)) if mount_type.is_gvfs_bridge() => match folders(&site.root).next()
        {
            // A bind mount of one backend's folder, or of a folder inside it.
            Some(backend) => MountNames::Profile(gvfs_bridge_profile(Some(backend))),
            None => MountNames::PerBackend {
                depth: u16::try_from(folders(&site.point).count()).unwrap_or(u16::MAX),
            },
        },
        _ => {
            let name_max = fs.statfs(dir).map_or(0, |stat| stat.name_max);
            MountNames::Profile(mount_type.map_or_else(
                || NameProfile::unknown(name_max),
                |mount| mount.name_profile(format, name_max),
            ))
        }
    };
    let symlinks = mount_type.and_then(|mount| holds_symlinks(&mount.fs_type, format));
    let max_file_size = mount_type.and_then(|mount| max_file_size(&mount.fs_type, format));
    let mut learned = MountCapabilities::default();
    learn_mount_capability(key, |capabilities| {
        if capabilities.family.is_none() {
            capabilities.family = Some(family);
            capabilities.identity =
                with_links(Some(family), identity, capabilities.links_keep_numbers);
            capabilities.fuse_format = fuse_format;
            capabilities.coarse_times = coarse_times;
            capabilities.deferred_close = deferred_close;
            capabilities.silly_names = silly_names;
            capabilities.attribute_cache = attribute_cache;
            capabilities.cuts_long_names_renamed_over = cuts_long_names_renamed_over;
            capabilities.names = Some(names);
            capabilities.symlinks = symlinks;
            capabilities.max_file_size = max_file_size;
        }
        learned = *capabilities;
    });
    learned
}

fn folders(path: &Path) -> impl Iterator<Item = &OsStr> {
    path.components().filter_map(|component| match component {
        Component::Normal(folder) => Some(folder),
        _ => None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MountProfile {
    pub names: NameProfile,
    pub strictness: Strictness,
    pub symlinks: Option<bool>,
    pub max_file_size: Option<u64>,
    pub family: Family,
    pub fuse_format: Option<FuseFormat>,
}

impl MountProfile {
    fn of(seen: MountCapabilities, site: &Site, taught: MountCapabilities) -> Self {
        Self {
            names: site.rules,
            strictness: taught.strictness,
            symlinks: taught.symlinks,
            max_file_size: seen.max_file_size,
            family: seen.family.unwrap_or(Family::Other),
            fuse_format: seen.fuse_format,
        }
    }

    fn unknown() -> Self {
        Self {
            names: NameProfile::unknown(0),
            strictness: Strictness::default(),
            symlinks: None,
            max_file_size: None,
            family: Family::Other,
            fuse_format: None,
        }
    }

    pub(super) fn gvfs(scheme: &str) -> Self {
        Self {
            names: gvfs_profile(scheme),
            strictness: Strictness::default(),
            symlinks: None,
            max_file_size: None,
            family: Family::Other,
            fuse_format: None,
        }
    }

    pub(super) fn windows_disk(&self) -> bool {
        matches!(self.family, Family::Fat | Family::Ntfs3) || self.fuse_format.is_some()
    }
}

/// Each mount is read once per operation, so its planner and workers agree even when a mount
/// change drops what was learned.
#[derive(Clone, Default)]
pub(super) struct Profiles(Arc<Mutex<HashMap<MountKey, MountCapabilities>>>);

impl Profiles {
    pub(super) fn mount(
        &self,
        fs: &dyn FsOps,
        dir: BorrowedFd<'_>,
    ) -> (MountKey, MountCapabilities) {
        let key = fs.mount_key(dir);
        let seen = self.seen().get(&key).copied();
        let seen = seen.unwrap_or_else(|| {
            let capabilities = known(fs, dir, key);
            *self.seen().entry(key).or_insert(capabilities)
        });
        (key, seen)
    }

    #[cfg(test)]
    pub(super) fn of(&self, fs: &dyn FsOps, dir: BorrowedFd<'_>) -> MountProfile {
        let (key, seen) = self.mount(fs, dir);
        let site = self.site(key, seen.names, dir);
        MountProfile::of(seen, &site, self.taught(&site))
    }

    /// Pins a GVfs bridge backend's capabilities for the rest of the operation.
    pub(super) fn site(
        &self,
        key: MountKey,
        names: Option<MountNames>,
        dir: BorrowedFd<'_>,
    ) -> Site {
        let site = Site::of(key, names, dir);
        if site.key() != key {
            self.seen()
                .entry(site.key())
                .or_insert_with(|| mount_capabilities(site.key()));
        }
        site
    }

    fn taught(&self, site: &Site) -> MountCapabilities {
        self.seen().get(&site.key()).copied().unwrap_or_default()
    }

    /// Lasts for later operations too, until the mount changes.
    pub(super) fn learn(&self, site: &Site, strictness: Strictness) {
        self.teach(site, |capabilities| {
            capabilities.strictness = capabilities.strictness.and(strictness);
        });
    }

    pub(super) fn holds_links(&self, site: &Site) -> bool {
        self.taught(site).symlinks != Some(false)
    }

    /// Where names must be UTF-8, a link target that is not stays behind: SMB with POSIX
    /// extensions stores each malformed byte as `?`, and NTFS refuses it.
    pub(super) fn holds_link_to(&self, site: &Site, target: &OsStr) -> bool {
        let taught = self.taught(site);
        taught.symlinks != Some(false)
            && (target.to_str().is_some()
                || site.rules.taught(taught.strictness).encoding == NameEncoding::AnyBytes)
    }

    pub(super) fn refuse_links(&self, site: &Site) {
        self.teach(site, |capabilities| capabilities.symlinks = Some(false));
    }

    fn teach(&self, site: &Site, learn: impl Fn(&mut MountCapabilities)) {
        if let Some(seen) = self.seen().get_mut(&site.key()) {
            learn(seen);
        }
        teach_mount_capability(site.mount, site.key(), learn);
    }

    fn seen(&self) -> MutexGuard<'_, HashMap<MountKey, MountCapabilities>> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[derive(Clone)]
pub(super) struct NamePolicy {
    pub profiles: Profiles,
    pub mode: NameMode,
}

impl Default for NamePolicy {
    fn default() -> Self {
        Self::new(NameMode::Keep)
    }
}

impl NamePolicy {
    pub(super) fn new(mode: NameMode) -> Self {
        Self {
            profiles: Profiles::default(),
            mode,
        }
    }

    /// Names given back keep all but what no name on the mount can have.
    pub(super) fn applied(&self, rules: NameProfile, strictness: Strictness) -> NameProfile {
        match self.mode {
            NameMode::Fix => rules.taught(strictness),
            NameMode::Keep => rules.encoding_rules(),
        }
    }

    pub(super) fn in_folder(&self, site: &Site) -> FolderRules {
        let strictness = if site.rules.learns {
            self.profiles.taught(site).strictness
        } else {
            Strictness::default()
        };
        FolderRules {
            fixed: self.applied(site.rules, strictness),
            limit: site.rules.taught(strictness).limit,
            site: *site,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FolderRules {
    pub fixed: NameProfile,
    pub limit: NameLimit,
    pub site: Site,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FolderNames {
    pub rules: NameProfile,
    pub fixing: bool,
    pub links: bool,
}

/// The GVfs bridge serves every backend from one mount, so there a site is a backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Site {
    pub rules: NameProfile,
    pub mount: MountKey,
    /// `None` on the bridge where no backend is known.
    backend: Option<MountKey>,
}

impl Site {
    pub(super) fn of(mount: MountKey, names: Option<MountNames>, dir: BorrowedFd<'_>) -> Self {
        let (rules, backend) = match names {
            Some(MountNames::Profile(rules)) => (rules, Some(mount)),
            Some(MountNames::PerBackend { depth }) => {
                let path = opened_path(dir).ok();
                let folder = path
                    .as_deref()
                    .and_then(|path| folders(path).nth(usize::from(depth)));
                (
                    gvfs_bridge_profile(folder),
                    folder.map(|folder| backend_key(mount, folder)),
                )
            }
            None => (NameProfile::unknown(0), Some(mount)),
        };
        Self {
            rules,
            mount,
            backend,
        }
    }

    pub(super) fn key(&self) -> MountKey {
        self.backend.unwrap_or(self.mount)
    }
}

/// The bridge names a backend's folder `<scheme>:<details>`, such as `smb-share:server=…`.
fn backend_key(bridge: MountKey, folder: &OsStr) -> MountKey {
    let mut hasher = DefaultHasher::new();
    (bridge, folder).hash(&mut hasher);
    MountKey::Backend(hasher.finish())
}

pub(super) fn crosses(source: (u64, &Site), target: (u64, &Site)) -> bool {
    source.0 != target.0 || source.1.backend.is_none() || source.1.backend != target.1.backend
}

fn crosses_into(fs: &dyn FsOps, source: BorrowedFd<'_>, target: (u64, &Site)) -> bool {
    let Ok(stat) = fs.stat_fd(source) else {
        return true;
    };
    if stat.identity.device != target.0 {
        return true;
    }
    let (key, seen) = known_mount(fs, source);
    crosses(
        (stat.identity.device, &Site::of(key, seen.names, source)),
        target,
    )
}

pub(super) fn name_rules(fs: &dyn FsOps, dir: BorrowedFd<'_>) -> NameProfile {
    let (key, seen) = known_mount(fs, dir);
    let site = Site::of(key, seen.names, dir);
    site.rules.taught(mount_capabilities(site.key()).strictness)
}

pub(super) fn name_limit(fs: &dyn FsOps, dir: BorrowedFd<'_>) -> NameLimit {
    name_rules(fs, dir).limit
}
