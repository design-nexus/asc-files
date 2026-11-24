// SPDX-License-Identifier: MIT

use super::fs_quirks::Preset;
use super::*;

/// Legacy's new folder or new file through its provider, `count` at once, as the engine's
/// `ops::create` on as many threads, each told as the provider's event.
fn create(
    parent: &Path,
    name: &str,
    unique_name: bool,
    count: usize,
    directory: bool,
) -> Vec<OperationEvent> {
    let _guard = serial();
    let start = Arc::new(std::sync::Barrier::new(count));
    let threads: Vec<_> = (0..count)
        .map(|id| {
            let (parent, name, start) = (parent.to_path_buf(), name.to_owned(), start.clone());
            thread::spawn(move || {
                start.wait();
                let request_id = OperationRequestId(id as u64);
                match crate::ops::create(&parent, &name, directory, unique_name) {
                    Ok(made) if unique_name => OperationEvent::EntryCreated {
                        request_id,
                        location: Location::local(made),
                    },
                    Ok(_) => OperationEvent::Created { request_id },
                    Err(error) => OperationEvent::Failed {
                        request_id,
                        message: error.message().to_owned(),
                        password_failure: None,
                    },
                }
            })
        })
        .collect();
    threads
        .into_iter()
        .map(|thread| thread.join().expect("a creation"))
        .collect()
}

#[test]
fn unique_creation_retries_atomic_collisions_without_overwriting() {
    for directory in [false, true] {
        let fixture = tempfile::tempdir().expect("fixture");
        let name = if directory { "new folder" } else { "new file" };
        fs::write(fixture.path().join(name), b"keep").expect("existing file");
        std::os::unix::fs::symlink("missing", fixture.path().join(format!("{name} (1)")))
            .expect("dangling link");
        fs::create_dir(fixture.path().join(format!("{name} (2)"))).expect("existing folder");
        let events = create(fixture.path(), name, true, 2, directory);
        let created: HashSet<_> = events
            .into_iter()
            .map(|event| match event {
                OperationEvent::EntryCreated { location, .. } => location,
                other => panic!("unexpected creation event: {other:?}"),
            })
            .collect();
        assert_eq!(
            created,
            HashSet::from([
                Location::local(fixture.path().join(format!("{name} (3)"))),
                Location::local(fixture.path().join(format!("{name} (4)"))),
            ])
        );
        assert_eq!(
            fs::read(fixture.path().join(name)).expect("existing contents"),
            b"keep"
        );
        assert!(fixture.path().join(format!("{name} (1)")).is_symlink());
        assert!(fixture.path().join(format!("{name} (2)")).is_dir());
        for suffix in [3, 4] {
            let path = fixture.path().join(format!("{name} ({suffix})"));
            if directory {
                assert!(path.is_dir());
            } else {
                assert_eq!(fs::read(path).expect("empty new file"), b"");
            }
        }
    }
}

#[test]
fn unique_creation_uses_the_first_available_name() {
    for directory in [false, true] {
        let fixture = tempfile::tempdir().expect("fixture");
        let name = if directory { "new folder" } else { "new file" };
        fs::create_dir(fixture.path().join(format!("{name} (1)"))).expect("numbered folder");
        assert!(
            matches!(create(fixture.path(), name, true, 1, directory).as_slice(), [OperationEvent::EntryCreated { location, .. }] if location == &Location::local(fixture.path().join(name)))
        );
        fs::remove_dir(fixture.path().join(format!("{name} (1)"))).expect("release first suffix");
        fs::create_dir(fixture.path().join(format!("{name} (2)"))).expect("later suffix");
        assert!(
            matches!(create(fixture.path(), name, true, 1, directory).as_slice(), [OperationEvent::EntryCreated { location, .. }] if location == &Location::local(fixture.path().join(format!("{name} (1)"))))
        );
    }
}

#[test]
fn unique_creation_numbers_a_long_name_within_the_limit() {
    for directory in [false, true] {
        let fixture = tempfile::tempdir().expect("fixture");
        let name = "n".repeat(253);
        fs::write(fixture.path().join(&name), b"keep").expect("existing file");
        let numbered = fixture.path().join(format!("{} (1)", "n".repeat(251)));
        assert!(
            matches!(create(fixture.path(), &name, true, 1, directory).as_slice(), [OperationEvent::EntryCreated { location, .. }] if location == &Location::local(&numbered)),
            "directory: {directory}"
        );
        assert_eq!(numbered.is_dir(), directory);
    }
}

#[test]
fn exact_creation_reports_conflicts_and_unique_creation_rejects_invalid_names() {
    for directory in [false, true] {
        let fixture = tempfile::tempdir().expect("fixture");
        fs::write(fixture.path().join("existing"), b"keep").expect("existing item");
        assert!(matches!(
            create(fixture.path(), "existing", false, 1, directory).as_slice(),
            [OperationEvent::Failed { .. }]
        ));
        for name in ["", "   ", ".", "..", "nested/name", "nul\0name"] {
            assert!(matches!(
                create(fixture.path(), name, true, 1, directory).as_slice(),
                [OperationEvent::Failed { .. }]
            ));
        }
        assert_eq!(fs::read_dir(fixture.path()).expect("listing").count(), 1);
        assert_eq!(
            fs::read(fixture.path().join("existing")).expect("existing contents"),
            b"keep"
        );
        assert!(matches!(
            create(fixture.path(), "exact", false, 1, directory).as_slice(),
            [OperationEvent::Created { .. }]
        ));
        assert_eq!(fixture.path().join("exact").is_dir(), directory);
    }
}

#[test]
fn a_typed_name_the_location_would_store_otherwise_is_refused_before_anything_is_made()
-> Result<(), Box<dyn Error>> {
    let long_folder_name = "f".repeat(214);
    let rows: [(Preset, &str, bool, Option<String>); 6] = [
        (
            Quirks::vfat,
            "a:b",
            false,
            Some(
                rust_i18n::t!(
                    "Names in this location can’t contain %{characters}",
                    characters = crate::i18n::quoted(":")
                )
                .into_owned(),
            ),
        ),
        (
            Quirks::vfat,
            "notes.",
            false,
            Some(crate::i18n::tr(
                "Names in this location can’t end with a space or a period",
            )),
        ),
        (
            Quirks::vfat,
            "aux",
            true,
            Some(
                rust_i18n::t!(
                    "“%{name}” is a reserved name in this location",
                    name = "aux"
                )
                .into_owned(),
            ),
        ),
        (
            Quirks::cifs,
            &long_folder_name,
            true,
            Some(crate::i18n::tr("The name is too long")),
        ),
        (Quirks::cifs, &long_folder_name, false, None),
        (Quirks::ext4, "a:b", true, None),
    ];
    for (quirks, name, directory, refusal) in rows {
        let root = tempfile::tempdir()?;
        let _scope = FsQuirks::install(root.path(), quirks());

        let events = create(root.path(), name, false, 1, directory);

        match refusal {
            Some(refusal) => {
                assert!(
                    matches!(events.as_slice(), [OperationEvent::Failed { message, .. }] if *message == refusal),
                    "{name:?}: {events:?}"
                );
                assert!(names(root.path())?.is_empty(), "{name:?}");
            }
            None => {
                assert!(
                    matches!(events.as_slice(), [OperationEvent::Created { .. }]),
                    "{name:?}: {events:?}"
                );
                assert_eq!(names(root.path())?, [name], "{name:?}");
                assert_eq!(root.path().join(name).is_dir(), directory, "{name:?}");
            }
        }
    }
    Ok(())
}
