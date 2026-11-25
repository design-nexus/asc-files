// SPDX-License-Identifier: MIT
//! Custom actions: the user's own scripts and programs on the item menus, as legacy Files had
//! them (`docs/custom-actions.md` there). Ported from legacy: the manifest and its checks
//! (`model`).

pub mod model;

pub use model::{
    ACTION_SCHEMA_VERSION, ActionConditions, ActionDefinition, ActionError, ActionInput,
    ActionRuntime, ArgumentToken, ErrorPolicy, ExecutionMode, FOLDER_CONTENT_TYPE, InputKind,
    InterpreterFamily, MAX_ACTION_ID_CHARS, MAX_ACTION_NAME_CHARS, MenuPlacement, RunSpec,
    WorkingDirectory, expand_arguments, interpreter_family, suggest_id, valid_action_id,
};
