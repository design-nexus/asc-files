// SPDX-License-Identifier: MIT

//! Names the user types for an item, refused before anything changes where
//! the location would store another name.

use std::{os::fd::AsFd, path::Path};

use super::{
    fs_ops::fs,
    mount_profile::name_rules,
    names::fix::{Refusal, refusal},
    native_fs::open_local_parent_directory,
};
use crate::adapters::volume::NameProfile;

pub(super) fn refused(name: &str, rules: &NameProfile, new_folder: bool) -> Option<String> {
    Some(match refusal(name, rules, new_folder)? {
        Refusal::Characters(characters) => rust_i18n::t!(
            "Names in this location can’t contain %{characters}",
            characters = crate::i18n::list(
                characters
                    .into_iter()
                    .map(|character| crate::i18n::quoted(&shown(character)))
            )
        )
        .into_owned(),
        Refusal::LeadingSpace => crate::i18n::tr("Names in this location can’t start with a space"),
        Refusal::TrailingDotOrSpace => {
            crate::i18n::tr("Names in this location can’t end with a space or a period")
        }
        Refusal::Reserved(word) => {
            rust_i18n::t!("“%{name}” is a reserved name in this location", name = word).into_owned()
        }
        Refusal::TooLong => crate::i18n::tr("The name is too long"),
    })
}

/// Why the folder `folder` would not store `name` as typed, or None where it would.
pub(crate) fn refused_in(folder: &Path, name: &str, new_folder: bool) -> Option<String> {
    let dir = open_local_parent_directory(folder).ok()?;
    refused(name, &name_rules(fs(), dir.as_fd()), new_folder)
}

/// Its code point where it shows nothing of its own: control and format characters, spaces,
/// marks that join the character before, and private-use code points.
fn shown(character: char) -> String {
    let code = u32::from(character);
    let invisible = character.is_control()
        || character.is_whitespace()
        || unicode_normalization::char::is_combining_mark(character)
        || matches!(code, 0xAD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFEFF)
        || matches!(code, 0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD);
    if invisible {
        format!("U+{code:04X}")
    } else {
        character.to_string()
    }
}
