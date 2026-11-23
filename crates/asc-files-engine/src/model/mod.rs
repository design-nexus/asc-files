// SPDX-License-Identifier: MIT

use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub mod hidden_names;

/// A browsable destination. Native paths remain byte-safe and URI locations remain explicit.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum LocationKind {
    Native(Arc<Path>),
    Uri(Arc<str>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Location {
    kind: LocationKind,
}

fn path_has_encoded_slash(path: &str) -> bool {
    path.as_bytes().windows(3).any(|escape| {
        escape[0] == b'%' && escape[1] == b'2' && escape[2].eq_ignore_ascii_case(&b'f')
    })
}

pub(crate) fn rebase_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let suffix = path.strip_prefix(from).ok()?;
    Some(if suffix.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(suffix)
    })
}

/// Resolves `.` and `..` as text like GIO, without consulting the filesystem;
/// `..` stops at the root. Callers pass absolute paths.
pub(crate) fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

impl Location {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        let path: PathBuf = path.into();
        Self {
            kind: LocationKind::Native(path.into()),
        }
    }

    pub fn uri(uri: impl Into<String>) -> Self {
        let uri: String = uri.into();
        Self {
            kind: LocationKind::Uri(uri.into()),
        }
    }

    pub fn native_path(&self) -> Option<&std::path::Path> {
        match &self.kind {
            LocationKind::Native(path) => Some(path),
            LocationKind::Uri(_) => None,
        }
    }

    pub fn uri_value(&self) -> Option<&str> {
        match &self.kind {
            LocationKind::Native(_) => None,
            LocationKind::Uri(uri) => Some(uri),
        }
    }

    /// Directory operations must reject virtual children as well as the root.
    pub fn is_recent_location(&self) -> bool {
        self.uri_value()
            .is_some_and(|uri| uri_scheme_eq(uri, "recent"))
    }

    pub fn is_recent_root(&self) -> bool {
        // Avoid GFile here: GVfs backends can SIGSEGV when tests call File APIs concurrently.
        self.uri_value().is_some_and(|uri| {
            if !uri_scheme_eq(uri, "recent") {
                return false;
            }
            uri.split_once(':')
                .is_some_and(|(_, rest)| rest.trim_start_matches('/').is_empty())
        })
    }

    pub fn favorites() -> Self {
        Self::uri("favorites:///")
    }

    pub fn is_favorites_root(&self) -> bool {
        self.uri_value()
            .is_some_and(|uri| uri_scheme_eq(uri, "favorites"))
    }

    /// Recent and Favorites list items kept elsewhere: nothing is made or put in them.
    pub fn is_virtual_list(&self) -> bool {
        self.is_recent_location() || self.is_favorites_root()
    }

    pub fn is_absolute_native(&self) -> bool {
        self.native_path().is_some_and(std::path::Path::is_absolute)
    }

    /// Where an item lands when transferred into `destination` without renaming.
    pub fn transfer_target(&self, destination: &Self) -> Option<Self> {
        destination.child(&self.file_name()?)
    }

    /// `to` may be of the other kind, as in a move between a share and a local disk.
    pub fn rebase(&self, from: &Self, to: &Self) -> Option<Self> {
        if let (LocationKind::Native(path), LocationKind::Native(from), LocationKind::Native(to)) =
            (&self.kind, &from.kind, &to.kind)
        {
            return rebase_path(path, from, to).map(Self::local);
        }
        // Relocating between URI locations is a remote adapter's job.
        None
    }

    pub fn compare(&self, other: &Self) -> Ordering {
        match (&self.kind, &other.kind) {
            (LocationKind::Native(left), LocationKind::Native(right)) => left.cmp(right),
            (LocationKind::Uri(left), LocationKind::Uri(right)) => left.cmp(right),
            (LocationKind::Native(_), LocationKind::Uri(_)) => Ordering::Less,
            (LocationKind::Uri(_), LocationKind::Native(_)) => Ordering::Greater,
        }
    }

    pub fn contains_camera_photo_location(&self, location: &Self) -> bool {
        self.is_camera_photo_root()
            && self.uri_value().is_some_and(|uri| {
                // GIO's fallback URI implementation distinguishes a trailing root slash.
                location.is_within(self)
                    || location.is_within(&Self::uri(uri.trim_end_matches('/')))
            })
    }

    pub fn display_name(&self) -> String {
        if self.is_recent_root() {
            return crate::i18n::tr("Recent");
        }
        if self.is_favorites_root() {
            return crate::i18n::tr("Favorites");
        }
        if self.is_camera_photo_root() {
            return crate::i18n::tr("Photos");
        }
        match &self.kind {
            LocationKind::Native(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            LocationKind::Uri(uri) if uri.as_ref() == "trash:///" => crate::i18n::tr("Trash"),
            LocationKind::Uri(uri) if uri.as_ref() == "network:///" => crate::i18n::tr("Network"),
            LocationKind::Uri(uri) => self
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| {
                    uri.trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(uri)
                        .into()
                }),
        }
    }

    #[cfg(test)]
    pub fn breadcrumbs(&self) -> Vec<Self> {
        if self.is_recent_root() {
            return vec![self.clone()];
        }
        if let Some(path) = self.native_path() {
            let mut locations: Vec<_> = path.ancestors().map(Self::local).collect();
            locations.reverse();
            return locations;
        }
        let mut locations = vec![self.clone()];
        while let Some(parent) = locations.last().and_then(Self::parent) {
            if locations.contains(&parent) {
                break;
            }
            locations.push(parent);
        }
        locations.reverse();
        locations
    }
    /// The engine works on native paths; a URI location belongs to a remote adapter and has no
    /// parent, child or name here.
    pub fn parent(&self) -> Option<Self> {
        if self.is_recent_root() || self.is_favorites_root() {
            return None;
        }
        match &self.kind {
            LocationKind::Native(path) => {
                let parent = path.parent()?;
                (parent != path.as_ref()).then(|| Self::local(parent))
            }
            LocationKind::Uri(_) => None,
        }
    }

    pub fn file_name(&self) -> Option<OsString> {
        match &self.kind {
            LocationKind::Native(path) => path.file_name().map(OsString::from),
            LocationKind::Uri(_) => None,
        }
    }

    pub fn child(&self, name: &std::ffi::OsStr) -> Option<Self> {
        if name.is_empty() || matches!(name.as_encoded_bytes(), b"." | b"..") {
            return None;
        }
        if name.as_encoded_bytes().contains(&b'/') {
            return None;
        }
        match &self.kind {
            LocationKind::Native(path) => Some(Self::local(path.join(name))),
            LocationKind::Uri(_) => None,
        }
    }

    pub fn is_within(&self, other: &Self) -> bool {
        self.native_path()
            .zip(other.native_path())
            .is_some_and(|(path, parent)| path.starts_with(parent))
    }

    pub fn is_camera_photo_root(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderSort {
    /// Not remembered per folder (remote, Trash, virtual roots): the column keeps its own sort.
    Unremembered,
    /// Remembered per folder but not set: the default sort applies.
    Default,
    Saved(SortKey, SortDirection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortKey {
    /// Camera-library-local streaming order; never a saved folder default.
    DeviceOrder,
    /// Recent-library-local use time; never a saved folder default.
    Recency,
    Name,
    Type,
    Size,
    Modified,
}

impl SortKey {
    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "name" => Some(Self::Name),
            "size" => Some(Self::Size),
            "modified" => Some(Self::Modified),
            "type" => Some(Self::Type),
            _ => None,
        }
    }

    pub(crate) fn stored_name(self) -> Option<&'static str> {
        match self {
            Self::Name => Some("name"),
            Self::Size => Some("size"),
            Self::Modified => Some("modified"),
            Self::Type => Some("type"),
            Self::DeviceOrder | Self::Recency => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "ascending" => Some(Self::Ascending),
            "descending" => Some(Self::Descending),
            _ => None,
        }
    }

    pub(crate) fn stored_name(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewPreferences {
    pub show_hidden: bool,
    pub folders_first: bool,
    pub sort_key: SortKey,
    pub sort_direction: SortDirection,
}

impl ViewPreferences {
    pub fn sort(&self) -> (SortKey, SortDirection) {
        (self.sort_key, self.sort_direction)
    }

    pub fn set_sort(&mut self, (sort_key, sort_direction): (SortKey, SortDirection)) {
        self.sort_key = sort_key;
        self.sort_direction = sort_direction;
    }
}

impl Default for ViewPreferences {
    fn default() -> Self {
        Self {
            show_hidden: false,
            folders_first: true,
            sort_key: SortKey::Name,
            sort_direction: SortDirection::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EntryKind {
    Directory,
    DirectorySymbolicLink,
    File,
    FileSymbolicLink,
    SymbolicLink,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataValue<T> {
    Unknown,
    Known(T),
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEntry {
    pub location: Location,
    /// Physical source for virtual entries; `location` remains their operational identity.
    pub thumbnail_path: Option<PathBuf>,
    pub native_name: OsString,
    pub display_name: String,
    pub kind: EntryKind,
    pub size: MetadataValue<u64>,
    pub modified_unix_seconds: MetadataValue<i64>,
    pub recent_unix_seconds: MetadataValue<i64>,
    /// Registry handle for Recent rows; `location` instead identifies the real target.
    pub recent_uri: Option<String>,
    pub mode: MetadataValue<u32>,
    pub image_dimensions: MetadataValue<(u32, u32)>,
    pub child_count: MetadataValue<u64>,
    pub duration_seconds: MetadataValue<u64>,
    pub is_hidden: bool,
}

impl FileEntry {
    pub fn local_thumbnail_path(&self) -> Option<&Path> {
        self.location
            .native_path()
            .or(self.thumbnail_path.as_deref())
    }

    pub fn is_directory(&self) -> bool {
        matches!(
            self.kind,
            EntryKind::Directory | EntryKind::DirectorySymbolicLink
        )
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, EntryKind::File | EntryKind::FileSymbolicLink)
    }

    pub fn is_broken_symbolic_link(&self) -> bool {
        self.kind == EntryKind::SymbolicLink
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FolderColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Gray,
}

impl FolderColor {
    pub const ALL: [FolderColor; 7] = [
        FolderColor::Red,
        FolderColor::Orange,
        FolderColor::Yellow,
        FolderColor::Green,
        FolderColor::Blue,
        FolderColor::Purple,
        FolderColor::Gray,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Gray => "Gray",
        }
    }

    pub fn hex(self) -> &'static str {
        match self {
            Self::Red => "#e5484d",
            Self::Orange => "#f76b15",
            Self::Yellow => "#e5a50a",
            Self::Green => "#30a46c",
            Self::Blue => "#0090ff",
            Self::Purple => "#8e4ec6",
            Self::Gray => "#8b8d98",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "red" => Some(Self::Red),
            "orange" => Some(Self::Orange),
            "yellow" => Some(Self::Yellow),
            "green" => Some(Self::Green),
            "blue" => Some(Self::Blue),
            "purple" => Some(Self::Purple),
            "gray" | "grey" => Some(Self::Gray),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum FolderColorValue {
    Preset(FolderColor),
    Custom(String),
}

impl FolderColorValue {
    pub fn hex(&self) -> &str {
        match self {
            Self::Preset(color) => color.hex(),
            Self::Custom(hex) => hex.as_str(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let trimmed = s.trim();
        if let Some(preset) = FolderColor::from_name(trimmed) {
            Some(Self::Preset(preset))
        } else if trimmed.starts_with('#')
            && (trimmed.len() == 7 || trimmed.len() == 4 || trimmed.len() == 9)
            && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            Some(Self::Custom(trimmed.to_ascii_lowercase()))
        } else {
            None
        }
    }

    pub fn to_preference_string(&self) -> String {
        match self {
            Self::Preset(preset) => preset.name().to_ascii_lowercase(),
            Self::Custom(hex) => hex.to_ascii_lowercase(),
        }
    }
}

#[cfg(test)]
mod tests;

fn uri_scheme_eq(uri: &str, scheme: &str) -> bool {
    let Some((parsed, _)) = uri.split_once(':') else {
        return false;
    };
    let valid = parsed.starts_with(|c: char| c.is_ascii_alphabetic())
        && parsed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid && parsed.eq_ignore_ascii_case(scheme)
}
