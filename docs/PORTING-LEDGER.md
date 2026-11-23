# Porting ledger

The legacy app's behavioural tests (`adapters/local_operations/tests/**` at `nyx-legacy/nyx-files@36887fe`, 492
tests in 22 files plus the `fs_quirks` harness) move here file by file in Phase A8. They live in
`crates/asc-files-engine/src/adapters/local_operations/tests/`, driven by `tests.rs`, whose stand-ins keep the
legacy helpers' names: GLib futures became the engine's synchronous core, and legacy's paste through its
provider runs as a batch of `jobs` (`run_paste`), so these tests check the job runner itself.

Every legacy test is either ported and passing, ported and `#[ignore]`d with a reason, or listed below as not
ported with a reason. This list must be empty, or every entry justified, before Phase A ends.

## Progress

| File | Legacy tests | Ported, passing | Ignored | Not ported |
|---|---|---|---|---|
| `aliases.rs` | 11 | 8 | 0 | 3 |
| `file_copy.rs` | 29 | 28 | 1 | 0 |
| `folder_copy.rs` | 22 | 21 | 1 (upstream: run by another test) | 0 |
| `naming.rs` | 21 | 17 | 0 | 4 |
| `paths.rs` | 3 | 2 | 0 | 1 |
| Remaining 17 files | 406 | – | – | – |

## Ignored

| Test | Why |
|---|---|
| `file_copy::selected_names_of_a_linked_file_on_an_smb_share_are_opened_one_at_a_time` | Needs many small top-level items copied in parallel. Legacy's paste had a bulk path for this; `jobs` copies top-level items one at a time (the engine's parallel walk applies inside folders). **Gap:** a bulk path in `jobs`. |

## Not ported

| Test | Why |
|---|---|
| `aliases::identical_top_level_names_from_different_folders_are_numbered_and_reported` | Paste planning (`transfer_targets`): which items of one paste meet each other's names was decided before copying. Now the client chooses Keep both per item. Returns with a planning call for the Phase B conflict prompt. |
| `aliases::an_item_from_another_folder_meets_the_original_of_a_duplicate` | Same. |
| `aliases::same_named_items_from_three_folders_all_land_whatever_their_order_and_choice` | Same. |
| `naming::pasted_names_fixed_for_fat_are_planned_after_the_names_kept` | Same: the names a conflict prompt shows. |
| `naming::transfer_targets_resolve_trash_leaf_names` | `trash://` URIs belong to the remote/GVfs adapter. |
| `naming::a_typed_name_is_refused_for_the_first_thing_its_mount_would_change` | Validation of a typed name (legacy `typed_names.rs`). **Gap:** the API has no rename, new folder or new file yet; returns with them. |
| `naming::a_typed_name_refusal_quotes_each_character_and_gives_the_code_point_of_those_that_show_nothing` | Same. |
| `paths::validated_children_are_confined_to_native_and_uri_parents` | A typed name may not leave its folder; returns with rename/new folder. The URI half belongs to the remote adapter. |

## Gaps the port found, and what was done

- **Fixed:** a paste into the folder an item is in failed (its name is taken) where legacy made a numbered
  duplicate. `ops::copy` now duplicates there.
- **Fixed:** items of one paste whose names differ only in case (`Notes`, `notes`) on a case-insensitive drive: the
  later one was refused where legacy numbered it and reported an alias. `jobs` now does so (legacy
  `case_variants.rs`, ported), deciding before copying, so no source is read twice.
- **Added:** moves that keep both (`ops::move_keeping_both`, `Action::MoveKeepingBoth`), and a name per item
  (`jobs::Item::name`, `ops::copy_as`) for a conflict prompt's choice.
- **Open:** moves with Replace or Merge; a bulk path for many small items; rename, new folder and new file; a
  paste-planning call.

## Archives (legacy: 225 tests)

Out of scope for Phase A.
