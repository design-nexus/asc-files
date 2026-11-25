// SPDX-License-Identifier: MIT

use std::{ffi::OsString, fmt, path::Path, path::PathBuf, rc::Rc};

use super::model::{
    ActionDefinition, ActionError, ActionInput, ArgumentToken, InterpreterFamily, MenuPlacement,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionProgram {
    Script {
        interpreter: OsString,
        interpreter_arguments: Vec<OsString>,
        script: PathBuf,
        family: InterpreterFamily,
    },
    Command {
        program: OsString,
        arguments: Vec<ArgumentToken>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionAvailability {
    Available(ActionProgram),
    Unavailable { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionHandle {
    pub definition: ActionDefinition,
    pub directory: PathBuf,
    pub availability: ActionAvailability,
}

impl ActionHandle {
    pub fn id(&self) -> &str {
        &self.definition.id
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn program(&self) -> Option<&ActionProgram> {
        match &self.availability {
            ActionAvailability::Available(program) => Some(program),
            ActionAvailability::Unavailable { .. } => None,
        }
    }

    pub fn is_available(&self) -> bool {
        self.program().is_some()
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        match &self.availability {
            ActionAvailability::Unavailable { reason } => Some(reason),
            ActionAvailability::Available(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionLoadFailure {
    /// Directory name, which may not be a valid action id.
    pub directory: String,
    pub error: ActionStoreError,
}

#[derive(Clone, Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "some of it is used only by tests")
)]
pub struct MatchedAction {
    pub action: Rc<ActionHandle>,
    pub placement: MenuPlacement,
}

#[derive(Clone, Debug, Default)]
pub struct ActionCatalog {
    actions: Vec<Rc<ActionHandle>>,
    failures: Vec<ActionLoadFailure>,
}

impl ActionCatalog {
    pub fn new(actions: Vec<Rc<ActionHandle>>, failures: Vec<ActionLoadFailure>) -> Self {
        Self { actions, failures }
    }

    fn stored(stored: StoredActions) -> Self {
        Self::new(
            stored.actions.into_iter().map(Rc::new).collect(),
            stored.failures,
        )
    }

    pub fn actions(&self) -> &[Rc<ActionHandle>] {
        &self.actions
    }

    pub fn failures(&self) -> &[ActionLoadFailure] {
        &self.failures
    }

    #[cfg(test)]
    pub fn get(&self, id: &str) -> Option<&Rc<ActionHandle>> {
        self.actions.iter().find(|action| action.id() == id)
    }

    pub fn matches(&self, inputs: &[ActionInput]) -> Vec<MatchedAction> {
        let mut matched: Vec<_> = self
            .actions
            .iter()
            .filter(|action| action.definition.enabled && action.definition.when.matches(inputs))
            .map(|action| MatchedAction {
                action: action.clone(),
                placement: action.definition.menu,
            })
            .collect();
        matched.sort_by(|left, right| {
            left.action
                .name()
                .to_lowercase()
                .cmp(&right.action.name().to_lowercase())
                .then_with(|| left.action.id().cmp(right.action.id()))
        });
        matched
    }

    #[cfg(test)]
    pub fn numbered(&self, inputs: &[ActionInput]) -> Vec<Rc<ActionHandle>> {
        let (top, submenu): (Vec<_>, Vec<_>) = self
            .matches(inputs)
            .into_iter()
            .filter(|matched| matched.action.is_available())
            .partition(|matched| matched.placement == MenuPlacement::Top);
        top.into_iter()
            .chain(submenu)
            .map(|matched| matched.action)
            .take(NUMBERED_ACTIONS)
            .collect()
    }
}

#[cfg(test)]
const NUMBERED_ACTIONS: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionScript {
    pub file_name: String,
    pub contents: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionWriteRequest {
    pub definition: ActionDefinition,
    /// `None` preserves the stored script during metadata-only edits.
    pub script: Option<ActionScript>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionStoreError {
    Invalid(ActionError),
    Io(String),
    AlreadyExists(String),
    NotFound(String),
    NotARegularFile(String),
    IdMismatch { declared: String, directory: String },
    MissingEntrypoint(String),
    MissingProgram(String),
    MissingInterpreter(String),
    NotAnActionDirectory(String),
}

impl fmt::Display for ActionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => error.fmt(formatter),
            Self::Io(message) => write!(formatter, "{message}"),
            Self::AlreadyExists(name) => {
                write!(formatter, "An action named “{name}” already exists")
            }
            Self::NotFound(id) => write!(formatter, "The action “{id}” no longer exists"),
            Self::IdMismatch {
                declared,
                directory,
            } => write!(
                formatter,
                "The manifest declares id \u{201c}{declared}\u{201d}, but its folder is named \u{201c}{directory}\u{201d}; they must match"
            ),
            Self::NotARegularFile(name) => write!(
                formatter,
                "“{name}” must be a regular file, not a link or directory"
            ),
            Self::MissingEntrypoint(name) => {
                write!(
                    formatter,
                    "The script “{name}” is missing beside action.toml"
                )
            }
            Self::MissingProgram(program) => {
                write!(formatter, "“{program}” was not found on your PATH")
            }
            Self::MissingInterpreter(program) => write!(
                formatter,
                "The interpreter “{program}” was not found. Install it, or point the action at an interpreter that exists"
            ),
            Self::NotAnActionDirectory(path) => {
                write!(formatter, "“{path}” does not contain an action.toml file")
            }
        }
    }
}

impl ActionStoreError {
    /// Localized counterpart of the English `Display`, which stays for logs.
    pub fn user_message(&self) -> String {
        use rust_i18n::t;
        match self {
            Self::Invalid(error) => return error.user_message(),
            Self::Io(message) => return message.clone(),
            Self::AlreadyExists(name) => {
                t!("An action named “%{name}” already exists", name = name)
            }
            Self::NotFound(id) => t!("The action “%{id}” no longer exists", id = id),
            Self::IdMismatch {
                declared,
                directory,
            } => t!(
                "The manifest declares id “%{declared}”, but its folder is named “%{directory}”; they must match",
                declared = declared,
                directory = directory
            ),
            Self::NotARegularFile(name) => t!(
                "“%{name}” must be a regular file, not a link or directory",
                name = name
            ),
            Self::MissingEntrypoint(name) => t!(
                "The script “%{name}” is missing beside action.toml",
                name = name
            ),
            Self::MissingProgram(program) => {
                t!("“%{program}” was not found on your PATH", program = program)
            }
            Self::MissingInterpreter(program) => t!(
                "The interpreter “%{program}” was not found. Install it, or point the action at an interpreter that exists",
                program = program
            ),
            Self::NotAnActionDirectory(path) => t!(
                "“%{path}” does not contain an action.toml file",
                path = path
            ),
        }
        .into_owned()
    }
}

impl std::error::Error for ActionStoreError {}

impl From<ActionError> for ActionStoreError {
    fn from(error: ActionError) -> Self {
        Self::Invalid(error)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StoredActions {
    pub actions: Vec<ActionHandle>,
    pub failures: Vec<ActionLoadFailure>,
}

impl StoredActions {
    #[cfg(test)]
    pub fn get(&self, id: &str) -> Option<&ActionHandle> {
        self.actions.iter().find(|action| action.id() == id)
    }
}

pub trait ActionStore: Send + Sync {
    /// A broken definition must not hide unrelated actions.
    fn load(&self) -> StoredActions;

    fn locate(&self, stored: StoredActions) -> StoredActions;

    fn definition(&self, id: &str) -> Result<ActionDefinition, ActionStoreError>;

    fn read(&self, id: &str) -> Result<ActionWriteRequest, ActionStoreError>;

    fn create(&self, request: &ActionWriteRequest) -> Result<(), ActionStoreError>;

    fn write(&self, request: &ActionWriteRequest) -> Result<(), ActionStoreError>;

    fn delete(&self, id: &str) -> Result<(), ActionStoreError>;

    fn read_import(&self, source: &Path) -> Result<ActionWriteRequest, ActionStoreError>;

    fn import(&self, request: ActionWriteRequest) -> Result<String, ActionStoreError>;

    fn export(
        &self,
        request: &ActionWriteRequest,
        destination: &Path,
    ) -> Result<PathBuf, ActionStoreError>;
}

/// Ids for a copy of `id`: `id-copy`, then `id-copy-1`, …, each within the id's length.
pub fn copy_ids(id: &str) -> impl Iterator<Item = String> + use<'_> {
    (0..1000)
        .map(move |suffix| {
            let suffix = if suffix == 0 {
                "-copy".to_owned()
            } else {
                format!("-copy-{suffix}")
            };
            let stem: String = id
                .chars()
                .take(super::model::MAX_ACTION_ID_CHARS - suffix.len())
                .collect();
            format!("{stem}{suffix}")
        })
        .filter(move |candidate| candidate != id)
}
