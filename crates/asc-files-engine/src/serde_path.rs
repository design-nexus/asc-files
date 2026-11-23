// SPDX-License-Identifier: MIT
//! Paths in serialized state: text when they are UTF-8, their bytes when not, so no name is lost
//! on the way to a file.

use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Stored {
    Text(String),
    Bytes { bytes: Vec<u8> },
}

pub(crate) fn stored(path: &Path) -> Stored {
    match path.to_str() {
        Some(text) => Stored::Text(text.to_owned()),
        None => Stored::Bytes {
            bytes: path.as_os_str().as_bytes().to_vec(),
        },
    }
}

pub(crate) fn loaded(stored: Stored) -> PathBuf {
    match stored {
        Stored::Text(text) => PathBuf::from(text),
        Stored::Bytes { bytes } => PathBuf::from(OsString::from_vec(bytes)),
    }
}

pub(crate) fn serialize<S: serde::Serializer>(path: &Path, to: S) -> Result<S::Ok, S::Error> {
    stored(path).serialize(to)
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(from: D) -> Result<PathBuf, D::Error> {
    Stored::deserialize(from).map(loaded)
}

pub(crate) mod pairs {
    use super::{Stored, *};

    pub(crate) fn serialize<S: serde::Serializer>(
        pairs: &[(PathBuf, PathBuf)],
        to: S,
    ) -> Result<S::Ok, S::Error> {
        pairs
            .iter()
            .map(|(a, b)| (super::stored(a), super::stored(b)))
            .collect::<Vec<_>>()
            .serialize(to)
    }

    pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
        from: D,
    ) -> Result<Vec<(PathBuf, PathBuf)>, D::Error> {
        Vec::<(Stored, Stored)>::deserialize(from).map(|pairs| {
            pairs
                .into_iter()
                .map(|(a, b)| (super::loaded(a), super::loaded(b)))
                .collect()
        })
    }
}
