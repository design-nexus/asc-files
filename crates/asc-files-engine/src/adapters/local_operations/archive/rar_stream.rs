// SPDX-License-Identifier: MIT
//! A RAR archive's members, read by the separately licensed helper (`asc-files-rar`) in a
//! bubblewrap sandbox of its own and streamed back; the engine writes them (legacy's
//! `sandbox/archive.rs`). The helper sees the archive, read-only, and nothing else of the user's;
//! where bubblewrap cannot make a sandbox it runs limited, as Files' other decoders do.

use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use super::rar_wire as wire;

// Bulk extraction needs more CPU than a preview render.
const RAR_CPU_TIME_LIMIT_SECS: u64 = 120;
// Limit stalls, not the total time spent extracting a large archive.
const READ_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);
const ADDRESS_SPACE_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Where the archive is in the sandbox.
const SANDBOX_INPUT: &str = "/input/archive.rar";

pub(crate) enum Member<'a> {
    Directory,
    File { size: u64, body: &'a mut dyn Read },
}

/// Callback errors lose their kind; only decoder failures retain it across the wire.
pub(crate) fn stream_rar(
    archive_path: &Path,
    password: Option<&str>,
    cancelled: &AtomicBool,
    on_member: impl FnMut(&str, Member<'_>, wire::WireMetadata) -> Result<(), String>,
) -> Result<(), wire::Failure> {
    let mut child = spawn(archive_path, password)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Missing RAR extraction pipe".to_owned())?;
    let reader = TimedPipeReader {
        fd: stdout,
        timeout: READ_INACTIVITY_TIMEOUT,
        cancelled,
    };
    let result = drive(reader, on_member);
    if result.is_err() {
        terminate(&mut child);
    } else {
        reap(&mut child, cancelled)?;
    }
    result
}

fn drive(
    mut reader: impl Read,
    mut on_member: impl FnMut(&str, Member<'_>, wire::WireMetadata) -> Result<(), String>,
) -> Result<(), wire::Failure> {
    wire::read_magic(&mut reader).map_err(|error| error.to_string())?;
    loop {
        let record = wire::read_record(&mut reader).map_err(|error| error.to_string())?;
        match record {
            wire::Record::End => return Ok(()),
            wire::Record::Error(failure) => return Err(failure),
            wire::Record::Directory(name, metadata) => {
                on_member(&name, Member::Directory, metadata)?;
            }
            wire::Record::File(name, size, metadata) => {
                let mut body = wire::FileBody::new(&mut reader, size);
                on_member(
                    &name,
                    Member::File {
                        size,
                        body: &mut body,
                    },
                    metadata,
                )?;
                std::io::copy(&mut body, &mut std::io::sink()).map_err(wire::Failure::from_io)?;
            }
        }
    }
}

fn reap(child: &mut Child, cancelled: &AtomicBool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            terminate(child);
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err("The sandboxed RAR extraction helper failed".to_owned())
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                terminate(child);
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate(child);
                return Err(format!(
                    "Unable to monitor the RAR extraction helper: {error}"
                ));
            }
        }
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The helper: `ASC_FILES_RAR`, else beside the running program, else on the `PATH`.
pub fn helper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ASC_FILES_RAR")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    let beside = std::env::current_exe()
        .ok()?
        .with_file_name("asc-files-rar");
    if beside.is_file() {
        return Some(beside);
    }
    on_path("asc-files-rar")
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Whether bubblewrap can make a sandbox here (it cannot inside some containers); asked once.
fn sandboxed() -> Option<PathBuf> {
    static BWRAP: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BWRAP
        .get_or_init(|| {
            let bwrap = ["/usr/bin/bwrap", "/run/current-system/sw/bin/bwrap"]
                .iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
                .or_else(|| on_path("bwrap"))?;
            let probe = Command::new(&bwrap)
                .args(sandbox_args())
                .args(["--", "/bin/true"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            probe.is_ok_and(|status| status.success()).then_some(bwrap)
        })
        .clone()
}

/// The sandbox: no network, no other process, no user files, the system read-only.
fn sandbox_args() -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--clearenv",
        "--setenv",
        "PATH",
        "/usr/bin",
        "--setenv",
        "MALLOC_ARENA_MAX",
        "1",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]
    .map(OsString::from)
    .to_vec();
    for dir in [
        "/usr",
        "/lib",
        "/lib64",
        "/bin",
        "/sbin",
        "/nix/store",
        "/etc/ld.so.cache",
    ] {
        args.extend(["--ro-bind-try", dir, dir].map(OsString::from));
    }
    args
}

/// The helper's command: in bubblewrap when `bwrap` is given, the archive bound read-only and
/// nothing writable; the password, when there is one, is asked for on stdin, never put on argv.
fn rar_extraction_command(
    bwrap: Option<&Path>,
    helper: &Path,
    archive_path: &Path,
    password: bool,
) -> Command {
    let mut command = match bwrap {
        Some(bwrap) => {
            let mut command = Command::new(bwrap);
            command
                .args(sandbox_args())
                .arg("--ro-bind")
                .arg(helper)
                .arg("/app/asc-files-rar")
                .arg("--ro-bind")
                .arg(archive_path)
                .arg(SANDBOX_INPUT)
                .args(["--", "/app/asc-files-rar", "extract-rar", SANDBOX_INPUT]);
            command
        }
        None => {
            let mut command = Command::new(helper);
            command.arg("extract-rar").arg(archive_path).env_clear();
            command
        }
    };
    if password {
        command.arg("--password-stdin");
    }
    command.stdin(if password {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    command
}

fn spawn(archive_path: &Path, password: Option<&str>) -> Result<Child, String> {
    let archive_path = archive_path
        .canonicalize()
        .map_err(|error| crate::services::io_error_message(&error))?;
    let helper = helper().ok_or_else(|| crate::i18n::tr("RAR support is not installed."))?;
    let mut command = rar_extraction_command(
        sandboxed().as_deref(),
        &helper,
        &archive_path,
        password.is_some(),
    );
    // SAFETY: only async-signal-safe calls (setrlimit) between fork and exec.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            use rustix::process::{Resource, Rlimit, setrlimit};
            let limit = |value: u64| Rlimit {
                current: Some(value),
                maximum: Some(value),
            };
            setrlimit(Resource::As, limit(ADDRESS_SPACE_LIMIT_BYTES))?;
            setrlimit(Resource::Cpu, limit(RAR_CPU_TIME_LIMIT_SECS))?;
            // It writes nothing: the members come back through the pipe.
            setrlimit(Resource::Fsize, limit(0))?;
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("The RAR helper could not be started: {error}"))?;
    if let Some(password) = password
        && let Some(mut stdin) = child.stdin.take()
    {
        // Through a pipe, never the command line; the helper reads it to the end.
        stdin
            .write_all(password.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(child)
}

struct TimedPipeReader<'a, F> {
    fd: F,
    timeout: Duration,
    cancelled: &'a AtomicBool,
}

impl<F: std::os::fd::AsFd> Read for TimedPipeReader<'_, F> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let deadline = Instant::now() + self.timeout;
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "Operation cancelled",
                ));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "RAR extraction progress timed out",
                ));
            }
            let mut fds = [PollFd::new(&self.fd, PollFlags::IN)];
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: remaining.min(Duration::from_millis(20)).as_nanos() as i64,
            };
            if poll(&mut fds, Some(&timeout))? != 0 {
                match rustix::io::read(&self.fd, &mut *buffer) {
                    Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => continue,
                    result => return result.map_err(Into::into),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
