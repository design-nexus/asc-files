// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn parent_resolution_accepts_absolute_relative_and_chained_aliases() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let actual = root.path().join("actual");
    fs::create_dir_all(actual.join("nested"))?;
    std::os::unix::fs::symlink(&actual, root.path().join("absolute"))?;
    std::os::unix::fs::symlink("actual", root.path().join("relative"))?;
    std::os::unix::fs::symlink("relative", root.path().join("chain"))?;
    std::os::unix::fs::symlink("../relative", actual.join("up"))?;
    let expected = super::LocalFileIdentity::from_stat(&rustix::fs::stat(&actual)?);

    for name in [
        "actual",
        "absolute",
        "relative",
        "chain",
        "actual/up",
        "chain/nested/..",
    ] {
        let parent = super::open_local_parent_directory(&root.path().join(name))?;
        assert_eq!(
            super::LocalFileIdentity::from_stat(&rustix::fs::fstat(&parent)?),
            expected,
            "{name}"
        );
    }
    let parent = super::open_local_parent_directory(Path::new("/"))?;
    assert_eq!(
        super::LocalFileIdentity::from_stat(&rustix::fs::fstat(&parent)?),
        super::LocalFileIdentity::from_stat(&rustix::fs::stat(c"/")?)
    );
    Ok(())
}

#[test]
fn parent_resolution_rejects_magic_links_loops_and_dangling_aliases() -> Result<(), Box<dyn Error>>
{
    use std::os::fd::AsRawFd;

    let root = tempfile::tempdir()?;
    let handle = fs::File::open(root.path())?;
    let magic = PathBuf::from(format!("/proc/self/fd/{}", handle.as_raw_fd()));
    assert!(
        magic.is_dir(),
        "the fixture must expose a working procfs magic link"
    );
    std::os::unix::fs::symlink(&magic, root.path().join("magic"))?;
    std::os::unix::fs::symlink("loop", root.path().join("loop"))?;
    std::os::unix::fs::symlink("missing", root.path().join("dangling"))?;

    for path in [
        magic,
        root.path().join("magic"),
        root.path().join("loop"),
        root.path().join("dangling"),
        PathBuf::from("relative"),
    ] {
        assert!(
            super::open_local_parent_directory(&path).is_err(),
            "{}",
            path.display()
        );
    }
    Ok(())
}
