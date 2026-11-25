// SPDX-License-Identifier: MIT

use std::{ffi::OsString, path::PathBuf, rc::Rc};

use super::super::model::{ActionDefinition, ActionInput, MenuPlacement};

use super::*;

fn definition(id: &str, name: &str, enabled: bool, menu: &str) -> ActionDefinition {
    ActionDefinition::parse(&format!(
        "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\nenabled = {enabled}\nmenu = \"{menu}\"\n\n[when]\nextensions = [\"png\"]\n\n[run]\nruntime = \"command\"\nprogram = \"true\"\nargs = [\"{{paths}}\"]\n"
    ))
    .expect("test definition is valid")
}

fn handle(id: &str, name: &str, enabled: bool, menu: MenuPlacement) -> Rc<ActionHandle> {
    let menu = match menu {
        MenuPlacement::Top => "top",
        MenuPlacement::Submenu => "submenu",
    };
    handle_from(&definition(id, name, enabled, menu))
}

fn handle_from(definition: &ActionDefinition) -> Rc<ActionHandle> {
    Rc::new(ActionHandle {
        definition: definition.clone(),
        directory: PathBuf::from("/tmp/actions").join(&definition.id),
        availability: ActionAvailability::Available(ActionProgram::Command {
            program: OsString::from("/usr/bin/true"),
            arguments: Vec::new(),
        }),
    })
}

#[test]
fn matches_only_enabled_actions_whose_rules_accept_every_input() {
    let catalog = ActionCatalog::new(
        vec![
            handle("a1", "Zebra", true, MenuPlacement::Top),
            handle("a2", "alpha", true, MenuPlacement::Submenu),
            handle("a3", "Disabled", false, MenuPlacement::Top),
        ],
        Vec::new(),
    );
    let images = [ActionInput::file("a.png", Some("image/png"))];
    let matched = catalog.matches(&images);
    assert_eq!(
        matched
            .iter()
            .map(|action| action.action.name())
            .collect::<Vec<_>>(),
        vec!["alpha", "Zebra"],
        "results are ordered by name and skip disabled actions"
    );
    assert_eq!(matched[0].placement, MenuPlacement::Submenu);
    assert_eq!(matched[1].placement, MenuPlacement::Top);

    assert!(
        catalog
            .matches(&[ActionInput::file("a.txt", None)])
            .is_empty(),
        "conditions must reject non-matching inputs"
    );
    assert!(catalog.matches(&[]).is_empty());
}

#[test]
fn numbered_actions_follow_the_menu_and_stop_at_ten() {
    let mut actions = vec![
        handle("sub", "Aardvark", true, MenuPlacement::Submenu),
        handle("top-z", "Zebra", true, MenuPlacement::Top),
        handle("top-a", "Apple", true, MenuPlacement::Top),
        handle("off", "Aaa off", false, MenuPlacement::Top),
    ];
    let mut unavailable = (*handle("broken", "Aaa broken", true, MenuPlacement::Top)).clone();
    unavailable.availability = ActionAvailability::Unavailable {
        reason: "missing".to_owned(),
    };
    actions.push(Rc::new(unavailable));
    actions.extend((0..9).map(|index| {
        handle(
            &format!("more-{index}"),
            &format!("More {index}"),
            true,
            MenuPlacement::Submenu,
        )
    }));
    let catalog = ActionCatalog::new(actions, Vec::new());
    let images = [ActionInput::file("a.png", Some("image/png"))];

    let numbered: Vec<_> = catalog
        .numbered(&images)
        .iter()
        .map(|action| action.id().to_owned())
        .collect();

    let mut expected = vec!["top-a", "top-z", "sub"];
    let more: Vec<String> = (0..7).map(|index| format!("more-{index}")).collect();
    expected.extend(more.iter().map(String::as_str));
    assert_eq!(
        numbered, expected,
        "top level first, runnable only, ten at most"
    );
    assert!(
        catalog
            .numbered(&[ActionInput::file("a.txt", None)])
            .is_empty()
    );
}
