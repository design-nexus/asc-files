use super::super::{
    deletion::{
        LocalDeleteJob, LocalDeleteQueue, PinnedEntry, parallel_delete_local_blocking_with_workers,
        process_local_delete_job, run_local_delete_workers,
    },
    native_fs::retry_local_open,
};
use super::fs_quirks::{Births, HoldGuard, Op, Racer, Trash, hold};
use super::*;
use crate::{
    model::hidden_names::foreign_leftover,
    services::{DeleteRequest, Holder, RestoreRequest, RestoreSource},
};
use std::sync::{Barrier, Mutex, atomic::Ordering, mpsc};

#[test]
fn permanent_deletion_reports_injected_unlink_failures_without_removing_the_item()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    for directory in [false, true] {
        let root = tempfile::tempdir()?;
        let path = root.path().join("item");
        if directory {
            fs::create_dir(&path)?;
        } else {
            fs::write(&path, b"keep")?;
        }
        let quirks = FsQuirks::install(
            root.path(),
            Quirks {
                fail: vec![(Op::Unlink, "item", 0, rustix::io::Errno::IO)],
                ..Quirks::default()
            },
        );
        let result =
            permanently_delete_local_path_if_unchanged(path.clone(), None, Cancellable::new());
        assert!(result.is_err());
        assert!(path.exists());
        assert_eq!(quirks.count(Op::Unlink), 1);
        if !directory {
            assert_eq!(fs::read(&path)?, b"keep");
        }
    }
    Ok(())
}

fn delete_root(path: &Path) -> Result<PinnedEntry, Box<dyn Error>> {
    Ok(PinnedEntry {
        parent: Arc::new(super::super::native_fs::open_local_parent_directory(
            path.parent().ok_or("no parent")?,
        )?),
        name: path.file_name().ok_or("no name")?.to_owned(),
        identity: None,
    })
}

fn enqueue_root(queue: &Arc<LocalDeleteQueue>, root: PinnedEntry) {
    queue.enqueue(LocalDeleteJob::Entry {
        parent: root.parent,
        name: root.name,
        expected: root.identity,
        guard: None,
        completion: None,
        deadline: Default::default(),
    });
}

fn process_next(queue: &Arc<LocalDeleteQueue>) {
    process_local_delete_job(queue, queue.next_job().expect("queued job"));
    queue.finish_job();
}

#[test]
fn permanent_delete_stops_if_an_open_directory_is_moved() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    let moved = root.path().join("moved");
    let sibling = root.path().join("sibling");
    fs::create_dir(&target)?;
    fs::write(target.join("child"), b"keep")?;
    fs::write(&sibling, b"keep sibling")?;
    let queue = Arc::new(LocalDeleteQueue::new(Arc::new(AtomicBool::new(false))));
    enqueue_root(&queue, delete_root(&sibling)?);
    enqueue_root(&queue, delete_root(&target)?);
    process_next(&queue);
    fs::rename(&target, &moved)?;
    fs::create_dir(&target)?;
    fs::write(target.join("replacement"), b"keep replacement")?;

    run_local_delete_workers(&queue, 1, |work| thread::Builder::new().spawn(work));

    let error = queue
        .result()
        .expect_err("moved directory must stop deletion");
    assert!(error.contains("changed"));
    assert_eq!(fs::read(moved.join("child"))?, b"keep");
    assert_eq!(fs::read(target.join("replacement"))?, b"keep replacement");
    assert_eq!(fs::read(sibling)?, b"keep sibling");
    Ok(())
}

#[test]
fn cancellation_after_enumeration_stops_and_joins_workers() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("first"), b"contents")?;
    fs::write(target.join("second"), b"contents")?;
    let cancellable = Cancellable::new();
    let cancel = cancellable.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(2));
    let cancel_barrier = barrier.clone();
    let canceller = thread::spawn(move || {
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker reached cancellation point");
        cancel.cancel();
        cancel_barrier.wait();
    });
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let error = parallel_delete_local_with(
        vec![delete_root(&target)?],
        cancellable,
        move |roots, cancelled| {
            let queue = Arc::new(LocalDeleteQueue::new(cancelled));
            for root in roots {
                enqueue_root(&queue, root);
            }
            process_next(&queue);
            process_next(&queue);
            ready_tx.send(()).expect("canceller is waiting");
            barrier.wait();
            run_local_delete_workers(&queue, 2, |work| {
                let finished = worker_finished.clone();
                thread::Builder::new().spawn(move || {
                    work();
                    finished.store(true, Ordering::Release);
                })
            });
            queue.result()
        },
    )
    .expect_err("in-flight delete must be cancelled");
    canceller.join().expect("canceller finished");
    assert!((error.kind() == ErrorKind::Cancelled));
    assert!(finished.load(Ordering::Acquire));
    assert_eq!(fs::read_dir(&target)?.count(), 1);
    assert!(!root.path().read_dir()?.any(|entry| {
        entry
            .expect("fixture entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".asc-files-trash-")
    }));
    Ok(())
}

#[test]
fn a_worker_start_failure_stops_and_joins_started_workers() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("keep");
    fs::write(&target, b"keep")?;
    let queue = Arc::new(LocalDeleteQueue::new(Arc::new(AtomicBool::new(false))));
    enqueue_root(&queue, delete_root(&target)?);
    let finished = Arc::new(AtomicBool::new(false));
    let mut attempts = 0;
    run_local_delete_workers(&queue, 2, |work| {
        attempts += 1;
        if attempts == 2 {
            return Err(io::Error::other("injected spawn failure"));
        }
        let queue = queue.clone();
        let finished = finished.clone();
        thread::Builder::new().spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !queue.is_stopped() {
                assert!(Instant::now() < deadline, "queue was not stopped");
                thread::yield_now();
            }
            work();
            finished.store(true, Ordering::Release);
        })
    });
    assert!(
        queue
            .result()
            .expect_err("worker start must fail")
            .contains("injected spawn failure")
    );
    assert!(finished.load(Ordering::Acquire));
    assert_eq!(fs::read(target)?, b"keep");
    Ok(())
}

#[test]
fn parallel_workers_finish_nested_trees_without_following_symlinks() -> Result<(), Box<dyn Error>> {
    for workers in [1, 2, 4] {
        let root = tempfile::tempdir()?;
        let target = root.path().join("target");
        let sentinel = root.path().join("sentinel");
        fs::write(&sentinel, b"keep")?;
        for index in 0..32 {
            let nested = target.join(format!("{index}/nested"));
            fs::create_dir_all(&nested)?;
            fs::write(nested.join("file"), b"delete")?;
            std::os::unix::fs::symlink(&sentinel, nested.join("link"))?;
        }
        parallel_delete_local_blocking_with_workers(
            vec![delete_root(&target)?],
            Arc::new(AtomicBool::new(false)),
            workers,
            &Default::default(),
        )?;
        assert!(!target.exists());
        assert_eq!(fs::read(sentinel)?, b"keep");
    }
    Ok(())
}

#[test]
fn transient_open_failures_retry_but_cannot_loop_forever() -> Result<(), Box<dyn Error>> {
    use rustix::io::Errno;
    for error in [Errno::AGAIN, Errno::INTR, Errno::ACCESS] {
        let mut attempts = 0;
        let result = retry_local_open(|| {
            attempts += 1;
            assert!(attempts <= 32, "unbounded retry");
            Err(error)
        });
        assert_eq!(result.expect_err("injected open failure"), error);
        if error == Errno::ACCESS {
            assert_eq!(attempts, 1);
        } else {
            assert!(attempts > 1);
        }
    }
    let mut attempts = 0;
    let fd = retry_local_open(|| {
        attempts += 1;
        match attempts {
            1 => Err(Errno::AGAIN),
            2 => Err(Errno::INTR),
            _ => rustix::fs::open(
                ".",
                rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
                rustix::fs::Mode::empty(),
            ),
        }
    })?;
    assert_eq!(attempts, 3);
    assert!(rustix::fs::fstat(fd).is_ok());
    Ok(())
}

#[test]
fn large_deletion_progress_reports_every_completed_location() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let mut entries = Vec::new();
    let mut expected = HashSet::new();
    for index in 0..40 {
        let path = root.path().join(format!("file-{index:02}.txt"));
        fs::write(&path, index.to_string())?;
        expected.insert(Location::local(&path));
        entries.push(file_entry(&path));
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(24),
            entries,
            permanent: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events
        .borrow()
        .iter()
        .any(|event| matches!(event, OperationEvent::Deleted { .. }))
    {
        pump();
    }

    let reported = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            OperationEvent::DeleteProgress {
                deleted_locations, ..
            } => Some(deleted_locations),
            _ => None,
        })
        .flatten()
        .cloned()
        .collect::<HashSet<_>>();
    assert_eq!(reported, expected);
    Ok(())
}

#[test]
fn a_cancelled_delete_counts_only_what_it_left_whole_as_not_attempted() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    // Legacy's Trash row cancelled inside GIO's trash, which checked its cancellable just before
    // renaming; the native Trash moves in one rename, so a cancel there lands as it moves.
    for (row, permanent, cut_is_folder) in [
        ("a file permanently", true, false),
        ("a folder permanently", true, true),
    ] {
        let root = tempfile::tempdir()?;
        let [denied, cut, rest] =
            ["denied.txt", "cut", "rest.txt"].map(|name| root.path().join(name));
        let (cut_entry, cut_inside) = if cut_is_folder {
            fs::create_dir(&cut)?;
            (directory_entry(&cut), cut.join("inside.txt"))
        } else {
            (file_entry(&cut), cut.clone())
        };
        for item in [&denied, &cut_inside, &rest] {
            fs::write(item, b"kept")?;
        }
        let cancellable = Cancellable::new();
        let cancel = cancellable.clone();
        let cancel: super::fs_quirks::Racer = Box::new(move || cancel.cancel());
        let cancel_at = if !permanent {
            Quirks {
                before: vec![(Op::Trash, "cut", 0, cancel)],
                ..Quirks::default()
            }
        } else {
            let checked = if cut_is_folder { "inside.txt" } else { "cut" };
            Quirks {
                after: vec![(Op::Stat, checked, 0, cancel)],
                ..Quirks::default()
            }
        };
        let _scope = FsQuirks::install(
            root.path(),
            Quirks {
                fail: vec![(Op::Stat, "denied.txt", 0, rustix::io::Errno::ACCESS)],
                ..cancel_at
            },
        );
        let events = Rc::new(RefCell::new(Vec::new()));
        let emitted = events.clone();

        run_deletion(
            OperationRequestId(1615),
            vec![file_entry(&denied), cut_entry, file_entry(&rest)],
            permanent,
            Rc::new(move |event| emitted.borrow_mut().push(event)),
            cancellable,
        );

        let events = events.borrow();
        let Some(OperationEvent::Cancelled { result, .. }) = events.last() else {
            panic!("{row}: expected a cancel, got {events:?}");
        };
        assert!(result.completed.is_empty(), "{row}");
        if permanent {
            assert!(
                !cut_inside.exists(),
                "{row}: the cancel landed before the delete"
            );
            assert_eq!(
                result.failed,
                [Location::local(&denied), Location::local(&cut)],
                "{row}"
            );
            assert_eq!(result.not_attempted, [Location::local(&rest)], "{row}");
        } else {
            assert_eq!(fs::read(&cut_inside)?, b"kept", "{row}");
            assert_eq!(result.failed, [Location::local(&denied)], "{row}");
            assert_eq!(
                result.not_attempted,
                [Location::local(&cut), Location::local(&rest)],
                "{row}"
            );
        }
        for item in [&denied, &rest] {
            assert_eq!(fs::read(item)?, b"kept", "{row}");
        }
    }
    Ok(())
}

#[test]
fn permanent_delete_removes_a_symlink_standing_in_for_a_directory_without_following_it()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let outside = std::env::temp_dir().join(format!("strata-delete-symlink-outside-{unique}"));
    let decoy = std::env::temp_dir().join(format!("strata-delete-symlink-decoy-{unique}"));
    fs::create_dir_all(&outside)?;
    let sentinel = outside.join("sentinel.txt");
    fs::write(&sentinel, b"do not delete me")?;
    // `directory_entry` reports `kind: Directory` even though the entry is
    // actually a symlink on disk, standing in for a `FileEntry` whose type
    // went stale because the real directory was swapped for a symlink.
    std::os::unix::fs::symlink(&outside, &decoy)?;

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(20),
            entries: vec![directory_entry(&decoy)],
            permanent: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events
        .borrow()
        .iter()
        .any(|event| matches!(event, OperationEvent::Deleted { .. }))
    {
        pump();
    }

    assert!(
        sentinel.exists(),
        "deleting the symlink must never touch what it points to"
    );
    assert_eq!(fs::read_dir(&outside)?.count(), 1);
    assert!(!decoy.exists() && !decoy.is_symlink());

    fs::remove_dir_all(outside)?;
    Ok(())
}

#[test]
fn permanent_delete_does_not_follow_a_symlink_nested_inside_the_tree() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("strata-delete-nested-symlink-test-{unique}"));
    let outside = std::env::temp_dir().join(format!("strata-delete-nested-outside-{unique}"));
    let nested = root.join("nested");
    fs::create_dir_all(&nested)?;
    fs::create_dir_all(&outside)?;
    let sentinel = outside.join("sentinel.txt");
    fs::write(&sentinel, b"do not delete me")?;
    fs::write(nested.join("visible.txt"), b"contents")?;
    std::os::unix::fs::symlink(&outside, nested.join("decoy"))?;

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(21),
            entries: vec![directory_entry(&root)],
            permanent: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events
        .borrow()
        .iter()
        .any(|event| matches!(event, OperationEvent::Deleted { .. }))
    {
        pump();
    }

    assert!(
        sentinel.exists(),
        "a symlink nested inside the deleted tree must never lead outside it"
    );
    assert_eq!(fs::read_dir(&outside)?.count(), 1);
    assert!(!root.exists());

    fs::remove_dir_all(outside)?;
    Ok(())
}

#[test]
fn permanent_delete_accepts_a_symlink_in_the_parent_path() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let actual_parent = root.path().join("actual");
    let linked_parent = root.path().join("linked");
    let target = actual_parent.join("target.txt");
    fs::create_dir(&actual_parent)?;
    fs::write(&target, b"keep")?;
    std::os::unix::fs::symlink(&actual_parent, &linked_parent)?;

    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(22),
            entries: vec![file_entry(&linked_parent.join("target.txt"))],
            permanent: true,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    while !events.borrow().iter().any(|event| {
        matches!(
            event,
            OperationEvent::Deleted { .. }
                | OperationEvent::CompletedWithErrors { .. }
                | OperationEvent::Failed { .. }
        )
    }) {
        pump();
    }

    assert!(
        events
            .borrow()
            .iter()
            .any(|event| matches!(event, OperationEvent::Deleted { .. })),
        "{:?}",
        events.borrow()
    );
    assert!(!target.exists());
    assert!(linked_parent.is_symlink());
    assert!(actual_parent.is_dir());
    Ok(())
}

#[test]
fn permanent_delete_keeps_the_open_parent_when_its_alias_is_retargeted()
-> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let actual = root.path().join("actual");
    let outside = root.path().join("outside");
    let alias = root.path().join("alias");
    fs::create_dir_all(actual.join("tree/nested"))?;
    fs::create_dir_all(outside.join("tree"))?;
    fs::write(actual.join("tree/nested/file.txt"), b"delete")?;
    fs::write(outside.join("tree/sentinel.txt"), b"keep")?;
    std::os::unix::fs::symlink(&outside, actual.join("tree/decoy"))?;
    std::os::unix::fs::symlink(&actual, &alias)?;
    let parent = super::open_local_parent_directory(&alias)?;

    std::os::unix::fs::symlink(&outside, root.path().join("replacement"))?;
    fs::rename(root.path().join("replacement"), &alias)?;
    permanently_delete_local(parent, OsString::from("tree"), None, Cancellable::new())?;

    assert!(!actual.join("tree").exists());
    assert_eq!(fs::read(outside.join("tree/sentinel.txt"))?, b"keep");
    assert_eq!(fs::read_link(alias)?, outside);
    Ok(())
}

#[test]
fn permanent_delete_revalidates_identity_after_a_parent_alias_changes() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let actual = root.path().join("actual");
    let outside = root.path().join("outside");
    let alias = root.path().join("alias");
    fs::create_dir(&actual)?;
    fs::create_dir(&outside)?;
    fs::write(actual.join("file.txt"), b"original")?;
    fs::write(outside.join("file.txt"), b"keep")?;
    std::os::unix::fs::symlink(&actual, &alias)?;
    let file = alias.join("file.txt");
    let expected = local_file_identity(&file)?;

    std::os::unix::fs::symlink(&outside, root.path().join("replacement"))?;
    fs::rename(root.path().join("replacement"), &alias)?;
    let error = (permanently_delete_local_path_if_unchanged(
        alias.join("file.txt"),
        expected,
        Cancellable::new(),
    ))
    .expect_err("retargeting the alias must not delete a different entry");

    assert!(error.to_string().contains("changed"), "{error}");
    assert_eq!(fs::read(actual.join("file.txt"))?, b"original");
    assert_eq!(fs::read(outside.join("file.txt"))?, b"keep");
    Ok(())
}

#[test]
fn a_removal_by_a_recorded_identity_follows_the_mounts_rule() -> Result<(), Box<dyn Error>> {
    #[derive(Clone, Copy, Debug)]
    enum Since {
        Evicted,
        Replaced,
        ReplacedAlike,
        ReplacedOnceNumbersWent,
    }
    let _serial = serial()?;
    let samba = || Quirks {
        births: Births::MadeUp,
        ..Quirks::cifs()
    };
    let rows: [(Preset, Since, bool); 5] = [
        (Quirks::vfat, Since::Evicted, true),
        (Quirks::vfat, Since::Replaced, false),
        (Quirks::ext4, Since::Evicted, false),
        (samba, Since::ReplacedAlike, false),
        (samba, Since::ReplacedOnceNumbersWent, false),
    ];
    for (preset, since, removed) in rows {
        for permanently in [true, false] {
            let row = format!("{:?}, {since:?}, {permanently}", preset().mount_type);
            let root = tempfile::tempdir()?;
            let path = root.path().join("item");
            let theirs = root.path().join("theirs");
            fs::write(&path, b"recorded")?;
            fs::write(&theirs, b"theirs")?;
            let hour_before = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)?
                .as_secs()
                - 3600;
            for file in [&path, &theirs] {
                set_times(file, hour_before, 0)?;
            }
            let _scope = FsQuirks::install(root.path(), preset());
            let expected = (local_file_identity(&path))?.ok_or("a local item")?;
            match since {
                Since::Evicted => assert!(super::fs_quirks::evict(&path), "{row}"),
                Since::Replaced => {
                    let probe = root.path().join("probe");
                    wait_for_a_later_birth_time(&probe, &fs::metadata(&path)?);
                    fs::remove_file(&path)?;
                    fs::write(&path, b"theirs")?;
                    super::fs_quirks::recreated(&path);
                }
                Since::ReplacedAlike => fs::rename(&theirs, &path)?,
                Since::ReplacedOnceNumbersWent => {
                    assert!(super::fs_quirks::autodisable_serverino(&path), "{row}");
                    fs::rename(&theirs, &path)?;
                }
            }

            let gone = if permanently {
                let result = permanently_delete_local_path_if_unchanged(
                    path.clone(),
                    Some(expected),
                    Cancellable::new(),
                );
                if let Err(error) = &result {
                    assert!(error.to_string().contains("changed"), "{row}: {error}");
                }
                result.is_ok()
            } else {
                let parent = super::open_local_parent_directory(root.path())?;
                let item = std::ffi::OsStr::new("item");
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| {
                            super::super::placement::remove_if_unchanged(
                                super::super::fs_ops::fs(),
                                super::super::fs_ops::Entry::new(&parent, item),
                                expected.identity,
                                expected.attributes,
                                rustix::fs::AtFlags::empty(),
                            )
                        })
                        .join()
                })
                .map_err(|_| "the removal panicked")?
            };

            assert_eq!(gone, removed, "{row}");
            assert_eq!(path.exists(), !removed, "{row}");
        }
    }
    Ok(())
}

#[test]
fn permanent_delete_of_a_symlink_through_an_alias_keeps_its_referent() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    let root = tempfile::tempdir()?;
    let actual = root.path().join("actual");
    let alias = root.path().join("alias");
    fs::create_dir(&actual)?;
    fs::write(actual.join("sentinel.txt"), b"keep")?;
    std::os::unix::fs::symlink("actual", &alias)?;
    std::os::unix::fs::symlink(".", actual.join("link"))?;

    permanently_delete_local_path_if_unchanged(alias.join("link"), None, Cancellable::new())?;

    assert!(!actual.join("link").is_symlink());
    assert_eq!(fs::read(actual.join("sentinel.txt"))?, b"keep");
    assert!(alias.is_symlink());
    Ok(())
}

#[test]
fn an_already_cancelled_recursive_delete_preserves_the_root() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let unique = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("strata-recursive-delete-cancel-test-{unique}"));
    let nested = root.join("nested");
    fs::create_dir_all(&nested)?;
    for index in 0..64 {
        fs::write(nested.join(format!("item-{index}.txt")), b"contents")?;
    }

    let cancellable = Cancellable::new();
    cancellable.cancel();
    let parent = super::open_local_parent_directory(root.parent().ok_or("no parent")?)?;
    let delete_root = PinnedEntry {
        parent: Arc::new(parent),
        name: root.file_name().ok_or("no name")?.to_owned(),
        identity: None,
    };
    let error = (parallel_delete_local(vec![delete_root], cancellable))
        .expect_err("cancelled delete must return error");

    assert!((error.kind() == ErrorKind::Cancelled));
    assert!(root.exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn trashing_and_restoring_report_each_items_identity_and_a_permanent_delete_reports_none() {
    crate::test_support::gtk_test(
        "adapters::local_operations::tests::deletion::trashing_and_restoring_report_each_items_identity_and_a_permanent_delete_reports_none",
        || {
            for permanent in [false, true] {
                let root = tempfile::tempdir().expect("root");
                let folder = root.path().join("folder");
                fs::create_dir(&folder).expect("folder");
                let identity = numbers_at(&folder);
                let events = Rc::new(RefCell::new(Vec::new()));
                let emitted = events.clone();
                let _operation = LocalOperationProvider.delete(
                    DeleteRequest {
                        id: OperationRequestId(95),
                        entries: vec![directory_entry(&folder)],
                        permanent,
                    },
                    Rc::new(move |event| emitted.borrow_mut().push(event)),
                );
                wait_for_operation(&events, |event| {
                    matches!(
                        event,
                        OperationEvent::Deleted { .. } | OperationEvent::CompletedWithErrors { .. }
                    )
                });

                let trashed = {
                    let events = events.borrow();
                    assert!(matches!(
                        events.last(),
                        Some(OperationEvent::Deleted { .. })
                    ));
                    assert_eq!(
                        trashed_items(&events),
                        if permanent {
                            Vec::new()
                        } else {
                            vec![(Location::local(&folder), identity)]
                        }
                    );
                    events.iter().find_map(|event| match event {
                        OperationEvent::ItemTrashed { identity, .. } => Some(*identity),
                        _ => None,
                    })
                };
                let Some(trashed) = trashed else {
                    continue;
                };
                let restoring = Rc::new(RefCell::new(Vec::new()));
                let emitted = restoring.clone();
                let _operation = LocalOperationProvider.restore(
                    RestoreRequest {
                        id: OperationRequestId(96),
                        source: RestoreSource::OriginalLocations {
                            locations: vec![Location::local(&folder)],
                            originals: HashMap::new(),
                        },
                    },
                    Rc::new(move |event| emitted.borrow_mut().push(event)),
                );
                wait_for_operation(&restoring, |event| {
                    matches!(
                        event,
                        OperationEvent::Restored { .. }
                            | OperationEvent::RestoreCompletedWithErrors { .. }
                            | OperationEvent::Failed { .. }
                    )
                });
                let restoring = restoring.borrow();
                let restored = restoring
                    .iter()
                    .filter_map(|event| match event {
                        OperationEvent::ItemRestored {
                            location, identity, ..
                        } => Some((location.clone(), *identity)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(
                    matches!(&restored[..], [(location, restored)]
                        if *location == Location::local(&folder) && restored.unchanged(&trashed)),
                    "{restored:?} after {:?}",
                    restoring.last()
                );
            }
        },
    );
}

#[derive(Clone, Copy, Debug)]
enum Swap {
    Untouched,
    RenamedOver,
    MovedAside,
}

#[test]
fn trash_reports_the_item_it_moved_when_another_took_the_name_first() {
    crate::test_support::gtk_test(
        "adapters::local_operations::tests::deletion::trash_reports_the_item_it_moved_when_another_took_the_name_first",
        || {
            // Legacy also swapped at the open it made of the item before trashing it; the native
            // Trash makes none, so what holds the name at the rename is what goes.
            // Legacy's nfs3 row expected the item it had checked: it read the item at its name
            // through NFS's attribute cache, which still showed it after another was renamed over.
            // The Trash moved the arrival, and the engine reports what reached the Trash.
            let rows: [(Preset, Op, Swap, bool); 3] = [
                (Quirks::ext4, Op::Trash, Swap::Untouched, false),
                (Quirks::ext4, Op::Trash, Swap::RenamedOver, true),
                (Quirks::sshfs, Op::Trash, Swap::MovedAside, true),
            ];
            for (preset, op, swap, reports_arrival) in rows {
                let root = tempfile::tempdir().expect("root");
                let item = root.path().join("report.txt");
                let arrival = root.path().join("arrival.txt");
                fs::write(&item, b"checked").expect("item");
                fs::write(&arrival, b"arrived later").expect("arrival");
                let (checked, arrived) = (numbers_at(&item), numbers_at(&arrival));
                let (from, to, aside) = (
                    arrival.clone(),
                    item.clone(),
                    root.path().join("report.txt~"),
                );
                let racer: Racer = Box::new(move || match swap {
                    Swap::Untouched => {}
                    Swap::RenamedOver => fs::rename(&from, &to).expect("renamed over"),
                    Swap::MovedAside => {
                        fs::rename(&to, &aside).expect("moved aside");
                        fs::rename(&from, &to).expect("renamed in");
                    }
                });
                let _quirks = FsQuirks::install(
                    root.path(),
                    Quirks {
                        before: vec![(op, "report.txt", 0, racer)],
                        ..preset()
                    },
                );
                let events = Rc::new(RefCell::new(Vec::new()));
                let emitted = events.clone();
                let _operation = LocalOperationProvider.delete(
                    DeleteRequest {
                        id: OperationRequestId(13),
                        entries: vec![file_entry(&item)],
                        permanent: false,
                    },
                    Rc::new(move |event| emitted.borrow_mut().push(event)),
                );
                wait_for_operation(&events, |event| {
                    matches!(
                        event,
                        OperationEvent::Deleted { .. } | OperationEvent::CompletedWithErrors { .. }
                    )
                });

                let events = events.borrow();
                assert!(
                    matches!(events.last(), Some(OperationEvent::Deleted { .. })),
                    "{op:?}, {swap:?}: {:?}",
                    events.last()
                );
                assert_eq!(
                    trashed_items(&events),
                    [(
                        Location::local(&item),
                        if reports_arrival { arrived } else { checked }
                    )],
                    "{:?}, {op:?}, {swap:?}",
                    preset().mount_type
                );
            }
        },
    );
}

const CLOCK_START: Duration = Duration::from_secs(1_800_000_000);

type Holds = Arc<std::sync::Mutex<Vec<HoldGuard>>>;

fn releasing(holds: &Holds) -> Racer {
    let holds = holds.clone();
    Box::new(move || holds.lock().expect("the holds").clear())
}

fn folder_with_an_open_file(path: &Path, holds: &Holds) -> io::Result<()> {
    fs::create_dir(path)?;
    fs::write(path.join("open.txt"), b"open elsewhere")?;
    let held = hold(&path.join("open.txt"));
    holds
        .lock()
        .map_err(|_| io::Error::other("the holds"))?
        .push(held);
    Ok(())
}

fn delete_permanently(folders: &[PathBuf]) -> Vec<OperationEvent> {
    delete(folders, true)
}

fn delete(folders: &[PathBuf], permanent: bool) -> Vec<OperationEvent> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let emitted = events.clone();
    let _operation = LocalOperationProvider.delete(
        DeleteRequest {
            id: OperationRequestId(1590),
            entries: folders
                .iter()
                .map(|folder| directory_entry(folder))
                .collect(),
            permanent,
        },
        Rc::new(move |event| emitted.borrow_mut().push(event)),
    );
    pump();
    events.take()
}

fn waited_since(start: SystemTime) -> Result<Duration, Box<dyn Error>> {
    Ok(super::super::copy_engine::clock_for_test().duration_since(start)?)
}

#[derive(Clone, Copy, Debug)]
enum Holding {
    Unnamed,
    Program,
}

#[derive(Clone, Copy, Debug)]
enum Meanwhile {
    StaysOpen(Holding),
    Closes,
    ClosesAsTheWaitEnds,
    IsSwappedForAnEmptyFolder,
}

const GONE_AFTER_THE_CLOSE: Duration = Duration::from_millis(100);

fn meanwhile_racer(
    meanwhile: Meanwhile,
    folder: &Path,
    holds: &Holds,
    wait_ends: SystemTime,
) -> Option<(Op, &'static str, usize, Racer)> {
    let (holds, folder) = (holds.clone(), folder.to_path_buf());
    match meanwhile {
        Meanwhile::StaysOpen(_) => None,
        Meanwhile::Closes => Some((Op::Unlink, "folder", 0, releasing(&holds))),
        Meanwhile::ClosesAsTheWaitEnds => Some((
            Op::Unlink,
            "folder",
            1,
            Box::new(move || {
                super::super::copy_engine::set_clock_for_test(Some(wait_ends));
                holds.lock().expect("the holds").clear();
                let gone = wait_ends + GONE_AFTER_THE_CLOSE;
                super::super::copy_engine::set_clock_for_test(Some(gone));
            }),
        )),
        Meanwhile::IsSwappedForAnEmptyFolder => Some((
            Op::Unlink,
            "folder",
            0,
            Box::new(move || {
                fs::rename(&folder, folder.with_extension("moved")).expect("move the folder");
                fs::create_dir(&folder).expect("make another folder");
            }),
        )),
    }
}

#[test]
fn a_folder_only_leftovers_keep_goes_once_they_do_within_the_mount_s_wait()
-> Result<(), Box<dyn Error>> {
    use Holding::*;
    use Meanwhile::*;
    let _serial = serial()?;
    let smb = Duration::from_millis(1500);
    let fuse_and_nfs = Duration::from_secs(2);
    let renamed = |holding| {
        [
            StaysOpen(Unnamed),
            StaysOpen(holding),
            Closes,
            ClosesAsTheWaitEnds,
            IsSwappedForAnEmptyFolder,
        ]
    };
    let rows: [(&str, Preset, Duration, &[Meanwhile]); 6] = [
        ("nfs3", Quirks::nfs3, fuse_and_nfs, &renamed(Program)),
        ("nfs42", Quirks::nfs42, fuse_and_nfs, &renamed(Program)),
        ("cifs", Quirks::cifs, smb, &renamed(Program)),
        (
            "cifs-posix",
            Quirks::cifs_posix,
            smb,
            &[
                StaysOpen(Unnamed),
                StaysOpen(Program),
                Closes,
                IsSwappedForAnEmptyFolder,
            ],
        ),
        ("sshfs", Quirks::sshfs, fuse_and_nfs, &renamed(Unnamed)),
        (
            "exfat-fuse",
            Quirks::exfat_fuse,
            fuse_and_nfs,
            &renamed(Unnamed),
        ),
    ];
    for (mount, preset, budget, cases) in rows {
        for &meanwhile in cases {
            let row = format!("{mount}, {meanwhile:?}");
            let start = SystemTime::UNIX_EPOCH + CLOCK_START;
            let _clock = ClockGuard::set(start);
            let root = tempfile::tempdir()?;
            let folder = root.path().join("folder");
            let holds = Holds::default();
            let holding = match meanwhile {
                StaysOpen(holding) => holding,
                _ => Unnamed,
            };
            let scope = FsQuirks::install(
                root.path(),
                Quirks {
                    after: meanwhile_racer(meanwhile, &folder, &holds, start + budget)
                        .into_iter()
                        .collect(),
                    holder: matches!(holding, Program)
                        .then(|| Holder::Program("Text Editor".to_owned())),
                    ..preset()
                },
            );
            folder_with_an_open_file(&folder, &holds)?;
            let real_folder = folder.canonicalize()?;
            let read_by_files = real_folder.join("open.txt");
            let events = delete_permanently(std::slice::from_ref(&folder));

            let waited = waited_since(start)?;
            let deleted = matches!(events.last(), Some(OperationEvent::Deleted { .. }));
            let leftovers_only = |folder: &Path| -> Result<bool, Box<dyn Error>> {
                let left = names(folder)?;
                Ok(left.len() <= 1
                    && left
                        .iter()
                        .all(|name| foreign_leftover(OsStr::new(name)).is_some()))
            };
            match meanwhile {
                Closes | ClosesAsTheWaitEnds => {
                    assert!(deleted, "{row}: {events:?}");
                    assert!(!folder.exists(), "{row}");
                    if let Closes = meanwhile {
                        assert!(waited < budget, "{row}: {waited:?}");
                    }
                }
                StaysOpen(holding) => {
                    assert!(!deleted, "{row}: {events:?}");
                    assert_eq!(waited, budget, "{row}");
                    assert!(leftovers_only(&folder)?, "{row}");
                    let reason = match (holding, mount) {
                        (Program, _) => rust_i18n::t!(
                            "A file in it is still open in %{program}. Close it there and try again.",
                            program = "Text Editor"
                        )
                        .into_owned(),
                        (Unnamed, "cifs") => crate::i18n::tr(
                            "A file in it is still open in a program on this or another computer.",
                        ),
                        (Unnamed, "cifs-posix") => crate::services::io_error_detail(
                            &io::Error::from_raw_os_error(rustix::io::Errno::ACCESS.raw_os_error()),
                        ),
                        (Unnamed, _) => crate::i18n::tr(
                            "A file in it is still open in another program. Close it and try again.",
                        ),
                    };
                    let line = rust_i18n::t!(
                        "Could not delete “%{name}”: %{error}",
                        name = "folder",
                        error = reason
                    );
                    assert!(
                        matches!(
                            events.last(),
                            Some(OperationEvent::CompletedWithErrors { message, .. })
                                if message.contains(&*line)
                        ),
                        "{row}: {events:?}"
                    );
                    let scanned = scope.count(Op::Holders);
                    assert_eq!(scanned, 1, "{row}");
                }
                IsSwappedForAnEmptyFolder => {
                    assert!(!deleted, "{row}: {events:?}");
                    assert!(names(&folder)?.is_empty(), "{row}");
                    assert!(leftovers_only(&folder.with_extension("moved"))?, "{row}");
                }
            }
        }
    }
    Ok(())
}

#[test]
fn one_wait_for_open_files_covers_every_folder_of_a_delete_or_trash() -> Result<(), Box<dyn Error>>
{
    let _serial = serial()?;
    let smb_folders_refused = || Quirks {
        folder_renames_refused_below: Some(Duration::from_secs(1)),
        ..Quirks::cifs()
    };
    let rows: [(&str, Preset, bool, Duration); 2] = [
        ("nfs3, Delete", Quirks::nfs3, true, Duration::from_secs(2)),
        (
            "cifs, Trash",
            smb_folders_refused,
            false,
            Duration::from_millis(1500),
        ),
    ];
    for (row, preset, permanent, budget) in rows {
        let start = SystemTime::UNIX_EPOCH + CLOCK_START;
        let _clock = ClockGuard::set(start);
        let root = tempfile::tempdir()?;
        let _scope = FsQuirks::install(root.path(), preset());
        let holds = Holds::default();
        let folders = [root.path().join("first"), root.path().join("second")];
        for folder in &folders {
            folder_with_an_open_file(folder, &holds)?;
        }

        let events = delete(&folders, permanent);

        assert!(
            matches!(
                events.last(),
                Some(OperationEvent::CompletedWithErrors { deleted_locations, .. })
                    if deleted_locations.is_empty()
            ),
            "{row}: {events:?}"
        );
        assert!(folders.iter().all(|folder| folder.is_dir()), "{row}");
        assert_eq!(waited_since(start)?, budget, "{row}");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Hidden {
    Leftover { closed_after: Option<Op> },
    LeftoverBesideAnEarlierOne,
    NamedLikeOne,
}

#[test]
fn a_recursive_delete_never_unlinks_another_program_s_leftover() -> Result<(), Box<dyn Error>> {
    let _serial = serial()?;
    let leftover = |closed_after| Hidden::Leftover { closed_after };
    let earlier = ".__smbfile_silly1";
    let rows: [(&str, Preset, Hidden); 10] = [
        ("nfs3", Quirks::nfs3, leftover(None)),
        ("nfs3", Quirks::nfs3, leftover(Some(Op::Unlink))),
        ("nfs3", Quirks::nfs3, leftover(Some(Op::List))),
        ("cifs", Quirks::cifs, leftover(None)),
        ("cifs", Quirks::cifs, leftover(Some(Op::Unlink))),
        ("cifs", Quirks::cifs, Hidden::LeftoverBesideAnEarlierOne),
        ("sshfs", Quirks::sshfs, leftover(None)),
        ("sshfs", Quirks::sshfs, leftover(Some(Op::Unlink))),
        ("ext4", Quirks::ext4, Hidden::NamedLikeOne),
        ("exfat-fuse", Quirks::exfat_fuse, Hidden::NamedLikeOne),
    ];
    for (mount, preset, hidden) in rows {
        let row = format!("{mount}, {hidden:?}");
        let start = SystemTime::UNIX_EPOCH + CLOCK_START;
        let _clock = ClockGuard::set(start);
        let root = tempfile::tempdir()?;
        let folder = root.path().join("folder");
        let holds = Holds::default();
        let closed_after = match hidden {
            Hidden::Leftover { closed_after } => closed_after,
            Hidden::LeftoverBesideAnEarlierOne | Hidden::NamedLikeOne => None,
        };
        let scope = FsQuirks::install(
            root.path(),
            Quirks {
                after: closed_after
                    .map(|op| (op, "folder", 0, releasing(&holds)))
                    .into_iter()
                    .collect(),
                ..preset()
            },
        );
        match hidden {
            Hidden::Leftover { .. } | Hidden::LeftoverBesideAnEarlierOne => {
                folder_with_an_open_file(&folder, &holds)?;
                if let Hidden::LeftoverBesideAnEarlierOne = hidden {
                    fs::write(folder.join(earlier), b"earlier")?;
                }
                let dir = open_local_parent_directory(&folder)?;
                super::super::fs_ops::fs().unlink(
                    super::super::fs_ops::Entry::new(&dir, OsStr::new("open.txt")),
                    rustix::fs::AtFlags::empty(),
                )?;
            }
            Hidden::NamedLikeOne => {
                fs::create_dir(&folder)?;
                fs::write(folder.join(".nfs000000000000005e000000de"), b"ordinary")?;
            }
        }
        let hidden_names = names(&folder)?;
        if let Hidden::LeftoverBesideAnEarlierOne = hidden {
            assert_eq!(hidden_names.len(), 2, "{row}: {hidden_names:?}");
            assert_eq!(fs::read(folder.join(earlier))?, b"earlier", "{row}");
        } else {
            assert_eq!(hidden_names.len(), 1, "{row}: {hidden_names:?}");
        }
        fs::write(folder.join("ordinary.txt"), b"ordinary")?;
        scope.clear_calls();

        let events = delete_permanently(std::slice::from_ref(&folder));

        let unlinked = scope
            .calls()
            .into_iter()
            .filter(|call| call.op == Op::Unlink)
            .flat_map(|call| call.names)
            .collect::<Vec<_>>();
        assert!(
            unlinked.contains(&"ordinary.txt".into()),
            "{row}: {unlinked:?}"
        );
        if let Hidden::NamedLikeOne = hidden {
            assert!(!folder.exists(), "{row}: {events:?}");
            continue;
        }
        assert!(
            hidden_names
                .iter()
                .all(|name| !unlinked.contains(&name.into())),
            "{row}: {unlinked:?}"
        );
        if closed_after.is_some() {
            assert!(
                matches!(events.last(), Some(OperationEvent::Deleted { .. })),
                "{row}: {events:?}"
            );
            assert!(!folder.exists(), "{row}");
        } else {
            assert!(
                matches!(
                    events.last(),
                    Some(OperationEvent::CompletedWithErrors { .. })
                ),
                "{row}: {events:?}"
            );
            assert_eq!(names(&folder)?, hidden_names, "{row}");
        }
    }
    Ok(())
}
