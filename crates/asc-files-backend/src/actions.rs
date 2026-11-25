// SPDX-License-Identifier: MIT
//! Custom actions over the protocol (`PROTOCOL.md`, Actions): the actions folder read and
//! edited for the app, and the queue that runs them. The queue (the engine's `JobService`,
//! legacy's, single-threaded) lives on a thread of its own, which takes commands from the
//! session, pumps the queue, and sends `action.job` whenever a job changes.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use asc_files_engine::actions::{
    catalog::{
        ActionCatalog, ActionHandle, ActionScript, ActionStore, ActionStoreError,
        ActionWriteRequest, copy_ids,
    },
    jobs::{InvocationSource, JobId, JobRequest, JobService, JobSnapshot, JobStatus},
    model::{
        ActionDefinition, ActionError, ActionInput, ExecutionMode, MenuPlacement,
        examples::ACTION_EXAMPLES,
    },
    run::LocalActionRunner,
    store::LocalActionStore,
};
use serde_json::{Value, json};

use crate::wire;

/// One selected item, as the app describes it for matching: its path, whether it is a
/// folder, and its type as the app guessed it.
#[derive(Clone, Debug)]
pub struct Input {
    pub path: PathBuf,
    pub folder: bool,
    pub content_type: Option<String>,
}

fn action_inputs(inputs: &[Input]) -> Vec<ActionInput> {
    inputs
        .iter()
        .map(|input| {
            let name = input
                .path
                .file_name()
                .map(ToOwned::to_owned)
                .unwrap_or_default();
            if input.folder {
                ActionInput::folder(name)
            } else {
                ActionInput::file(name, input.content_type.as_deref())
            }
        })
        .collect()
}

/// The actions folder, as the app reads and edits it.
pub struct Folder {
    store: Arc<LocalActionStore>,
}

impl Folder {
    pub fn new() -> Self {
        Self {
            store: LocalActionStore::new(),
        }
    }

    pub fn store(&self) -> Arc<LocalActionStore> {
        self.store.clone()
    }

    fn catalog(&self) -> ActionCatalog {
        ActionCatalog::from_stored(self.store.locate(self.store.load()))
    }

    /// Every action, the folders that couldn't be read, and (given `inputs`) the ids offered
    /// for them in menu order: top-level ones first.
    pub fn list(&self, inputs: Option<&[Input]>) -> Value {
        let catalog = self.catalog();
        let mut listed = json!({
            "actions": catalog.actions().iter().map(|action| action_json(action)).collect::<Vec<_>>(),
            "failures": catalog.failures().iter().map(|failure| json!({
                "directory": failure.directory,
                "reason": failure.error.user_message(),
            })).collect::<Vec<_>>(),
        });
        if let Some(inputs) = inputs {
            let matched = catalog.matches(&action_inputs(inputs));
            let (top, submenu): (Vec<_>, Vec<_>) = matched
                .into_iter()
                .partition(|matched| matched.placement == MenuPlacement::Top);
            listed["matched"] = json!(
                top.into_iter()
                    .chain(submenu)
                    .map(|matched| matched.action.id().to_owned())
                    .collect::<Vec<_>>()
            );
        }
        listed
    }

    /// An action's definition (its manifest, as JSON) and script, for the editor.
    pub fn read(&self, id: &str) -> Result<Value, ActionStoreError> {
        let request = self.store.read(id)?;
        Ok(json!({
            "definition": serde_json::to_value(&request.definition).unwrap_or(Value::Null),
            "script": request.script.map(|script| json!({"fileName": script.file_name, "contents": script.contents})),
        }))
    }

    /// Writes an action from the editor: a new one (`create`, never over another) or changes.
    pub fn write(
        &self,
        definition: &Value,
        script: Option<&Value>,
        create: bool,
    ) -> Result<String, ActionStoreError> {
        let definition: ActionDefinition = serde_json::from_value(definition.clone())
            .map_err(|error| ActionStoreError::Invalid(ActionError::Toml(error.to_string())))?;
        definition.validate()?;
        let script = script.and_then(|script| {
            Some(ActionScript {
                file_name: script.get("fileName")?.as_str()?.to_owned(),
                contents: script.get("contents")?.as_str()?.to_owned(),
            })
        });
        let request = ActionWriteRequest { definition, script };
        if create {
            self.store.create(&request)?;
        } else {
            self.store.write(&request)?;
        }
        Ok(request.definition.id)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ActionStoreError> {
        let mut request = self.store.read(id)?;
        request.definition.enabled = enabled;
        // The script stays as it is.
        request.script = None;
        self.store.write(&request)
    }

    pub fn delete(&self, id: &str) -> Result<(), ActionStoreError> {
        self.store.delete(id)
    }

    /// A copy beside it, disabled, under the first free `id-copy…` id; the new id.
    pub fn duplicate(&self, id: &str) -> Result<String, ActionStoreError> {
        let original = self.store.read(id)?;
        let mut last = None;
        for candidate in copy_ids(id) {
            let mut copy = original.clone();
            copy.definition.id = candidate.clone();
            copy.definition.name = format!("{} (copy)", original.definition.name)
                .chars()
                .take(64)
                .collect();
            copy.definition.enabled = false;
            match self.store.create(&copy) {
                Ok(()) => return Ok(candidate),
                Err(error @ ActionStoreError::AlreadyExists(_)) => last = Some(error),
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| ActionStoreError::AlreadyExists(id.to_owned())))
    }

    /// Copies an action folder in, disabled for review; the id it was given.
    pub fn import(&self, source: &Path) -> Result<String, ActionStoreError> {
        let request = self.store.read_import(source)?;
        self.store.import(request)
    }

    /// Writes a portable copy of `id` into the folder `destination`; where it went.
    pub fn export(&self, id: &str, destination: &Path) -> Result<PathBuf, ActionStoreError> {
        let request = self.store.read(id)?;
        self.store.export(&request, destination)
    }
}

/// The recipe library the editor offers.
pub fn examples() -> Value {
    json!(
        ACTION_EXAMPLES
            .iter()
            .map(|example| json!({
                "name": example.name,
                "category": example.category,
                "description": example.description,
                "requirements": example.requirements,
                "inputs": example.inputs,
                "extensions": example.extensions,
                "folders": example.folders,
                "mode": mode_name(example.mode),
                "runtime": serde_json::to_value(example.runtime()).unwrap_or(Value::Null),
                "script": example.script(),
            }))
            .collect::<Vec<_>>()
    )
}

fn mode_name(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::WholeSelection => "whole-selection",
        ExecutionMode::PerItem => "per-item",
    }
}

fn action_json(action: &ActionHandle) -> Value {
    let definition = &action.definition;
    json!({
        "id": definition.id,
        "name": definition.name,
        "description": definition.description,
        "icon": definition.icon,
        "enabled": definition.enabled,
        "menu": match definition.menu { MenuPlacement::Top => "top", MenuPlacement::Submenu => "submenu" },
        "runtime": serde_json::to_value(definition.run.runtime).unwrap_or(Value::Null),
        "mode": mode_name(definition.run.mode),
        "confirm": definition.run.confirm,
        "available": action.is_available(),
        "reason": action.unavailable_reason(),
    })
}

fn status_name(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Cancelling => "cancelling",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

fn snapshot_json(snapshot: &JobSnapshot) -> Value {
    let progress = &snapshot.progress;
    json!({
        "job": snapshot.id.0,
        "name": snapshot.action_name,
        "icon": snapshot.icon,
        "mode": mode_name(snapshot.mode),
        "parent": wire::path(&snapshot.parent),
        "status": status_name(snapshot.status),
        "completed": progress.completed_items,
        "succeeded": progress.succeeded_items,
        "failed": progress.failed_items,
        "total": progress.total_items,
        "fraction": progress.fraction(snapshot.mode),
        "step": progress.script.as_ref().and_then(|script| script.message.clone()).or_else(|| progress.message.clone()),
        "message": snapshot.message,
        "log": snapshot.log,
        "logTruncated": snapshot.log_truncated,
        "created": snapshot.created.iter().map(|path| wire::path(path)).collect::<Vec<_>>(),
        "elapsed": u64::try_from(snapshot.elapsed.as_millis()).unwrap_or(u64::MAX),
    })
}

enum Command {
    Run {
        id: String,
        inputs: Vec<Input>,
        parent: PathBuf,
        background: bool,
        reply: Sender<Result<u64, String>>,
    },
    Cancel(u64),
    Dismiss(u64),
}

/// The queue of action runs, on its own thread.
pub struct Queue {
    commands: Option<Sender<Command>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Queue {
    pub fn start(out: Sender<Value>, store: Arc<LocalActionStore>) -> Self {
        let (commands, received) = mpsc::channel::<Command>();
        let thread = thread::spawn(move || {
            let service = JobService::new(LocalActionRunner::new());
            // What each job last looked like when sent, and when.
            let mut sent: HashMap<u64, (Value, Instant)> = HashMap::new();
            let mut closing: Option<Instant> = None;
            loop {
                match received.recv_timeout(Duration::from_millis(100)) {
                    Ok(Command::Run {
                        id,
                        inputs,
                        parent,
                        background,
                        reply,
                    }) => {
                        let _ = reply
                            .send(enqueue(&service, &*store, &id, &inputs, parent, background));
                    }
                    Ok(Command::Cancel(job)) => {
                        service.cancel(JobId(job));
                    }
                    Ok(Command::Dismiss(job)) => {
                        service.dismiss(JobId(job));
                        sent.remove(&job);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        // The session is done: what is running is stopped, waited on a while.
                        if closing.is_none() {
                            closing = Some(Instant::now());
                            for snapshot in service.snapshot() {
                                if snapshot.is_active() {
                                    service.cancel(snapshot.id);
                                }
                            }
                        }
                    }
                }
                service.pump();
                let snapshots = service.snapshot();
                for snapshot in &snapshots {
                    let mut state = snapshot_json(snapshot);
                    let elapsed = state["elapsed"].take();
                    let changed = sent.get(&snapshot.id.0).is_none_or(|(last, at)| {
                        *last != state
                            || (snapshot.is_active() && at.elapsed() >= Duration::from_secs(1))
                    });
                    if changed {
                        sent.insert(snapshot.id.0, (state.clone(), Instant::now()));
                        state["elapsed"] = elapsed;
                        let _ = out.send(wire::notification("action.job", state));
                    }
                }
                if let Some(since) = closing
                    && (snapshots.iter().all(|snapshot| !snapshot.is_active())
                        || since.elapsed() > Duration::from_secs(5))
                {
                    break;
                }
            }
        });
        Self {
            commands: Some(commands),
            thread: Some(thread),
        }
    }

    pub fn run(
        &self,
        id: String,
        inputs: Vec<Input>,
        parent: PathBuf,
        background: bool,
    ) -> Result<u64, String> {
        let (reply, answer) = mpsc::channel();
        self.commands
            .as_ref()
            .ok_or_else(|| "Actions have stopped".to_owned())?
            .send(Command::Run {
                id,
                inputs,
                parent,
                background,
                reply,
            })
            .map_err(|_| "Actions have stopped".to_owned())?;
        answer
            .recv()
            .map_err(|_| "Actions have stopped".to_owned())?
    }

    pub fn cancel(&self, job: u64) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Cancel(job));
        }
    }

    pub fn dismiss(&self, job: u64) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Dismiss(job));
        }
    }

    /// Stops what is running and waits for it (a few seconds at most).
    pub fn finish(&mut self) {
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Queues `id` on `inputs`, read afresh: an action changed or removed since the menu offered it,
/// or no longer matching what is selected, does not run.
fn enqueue(
    service: &JobService,
    store: &dyn ActionStore,
    id: &str,
    inputs: &[Input],
    parent: PathBuf,
    background: bool,
) -> Result<u64, String> {
    let catalog = ActionCatalog::from_stored(store.locate(store.load()));
    let Some(action) = catalog.actions().iter().find(|action| action.id() == id) else {
        return Err(ActionStoreError::NotFound(id.to_owned()).user_message());
    };
    if !action.definition.enabled || !action.definition.when.matches(&action_inputs(inputs)) {
        return Err("The actions changed: this one doesn't take what is selected now".to_owned());
    }
    service
        .enqueue(JobRequest {
            action: action.clone(),
            inputs: inputs.iter().map(|input| input.path.clone()).collect(),
            parent,
            source: if background {
                InvocationSource::Background
            } else {
                InvocationSource::Selection
            },
        })
        .map(|job| job.0)
        .map_err(|error| error.to_string())
}
