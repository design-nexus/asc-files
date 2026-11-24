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
| `closes.rs` | 1 | 0 | 0 | 1 |
| `conflicts.rs` | 20 | 15 | 0 | 5 |
| `file_copy.rs` | 29 | 28 | 1 | 0 |
| `folder_copy.rs` | 22 | 21 | 1 (upstream: run by another test) | 0 |
| `moves.rs` | 51 | 42 | 0 | 9 |
| `naming.rs` | 21 | 17 | 0 | 4 |
| `paths.rs` | 3 | 2 | 0 | 1 |
| `placement.rs` | 39 | 37 | 0 | 2 |
| `replacement.rs` | 28 | 25 | 0 | 3 |
| `merge.rs`, first pass | 66 | 14 | 0 | 2 (50 still to port: legacy's merge hooks and seams, and its snapshot undo) |
| **So far** | **241** | **209** | **2** | **30** |
| Remaining 10 files, and 50 of `merge.rs` | 251 | – | – | – |

## Ignored

| Test | Why |
|---|---|
| `file_copy::selected_names_of_a_linked_file_on_an_smb_share_are_opened_one_at_a_time` | Needs many small top-level items copied in parallel. Legacy's paste had a bulk path for this; `jobs` copies top-level items one at a time (the engine's parallel walk applies inside folders). **Gap:** a bulk path in `jobs`. |

## Rows skipped inside ported tests

| Test, row | Why |
|---|---|
| `moves::a_moved_source_stays_when_its_copy_is_gone_before_the_deletion`, Replace published by exchange | A replacing move across drives sends the original to the Trash and then moves (`ops::move_replacing`); legacy staged a copy and exchanged it in. No exchange happens, so the row's racer cannot fire; the rows racing the sync cover the copy going. |
| `moves::moves_compare_inode_numbers_only_where_the_source_keeps_them`, the two "linked to an item moved first" rows | Legacy copied every item of a paste before deleting any source; jobs finish each item first. The lookalike the row puts in place lands before the folder is copied, so moving it is right. |
| `moves::a_moved_source_stays_when_its_copy_is_gone_before_the_deletion`, Replace published by exchange | After its exchange the engine proves the name holds its copy; one removed there at once stops the replace and puts the original back, with the source whole, where legacy carried on and kept the source. |
| `replacement::replacement_stops_before_exchanging_a_substituted_target`, the OpenAt row | Legacy held the target open before staging; the engine reads it by identity instead, so there is no open to substitute at. The row during the copy runs. |
| `replacement::a_target_is_replaced_only_while_its_mount_still_knows_it`, the Evicted rows (FAT, SFTP, SMB once inode numbers went) | **Open gap.** Where a mount renumbers an evicted original, legacy still knew it and replaced it; the engine refuses. The safe way round: every row that must be refused runs and is refused. |
| Same test, ReplacedLonger on a path-keyed SFTP mount | **Open gap.** Refused as it must be, but the item put in the original's place is left set aside (and named in the error) instead of put back. |
| `replacement::rejected_exchange_falls_back_to_aside_and_is_remembered`, the exchange count | Legacy skipped the exchange for a read-only staged folder; the engine tries it, is refused, and falls back. The outcome and what is remembered are checked. |
| `replacement::a_failed_publication_puts_the_original_back`, the FAT row | Re-aimed at the engine's publish (its second rename at the name on FAT; legacy's was its fourth). |

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
| `closes::what_an_operation_lets_go_of_is_closed_on_a_worker` | GIO objects closed off the UI thread: the engine has neither. |
| `conflicts::transfers_into_the_same_location_or_a_descendant_are_noops` | Legacy's planning helper `transfer_is_noop`; its cases run end to end in `cutting_in_the_same_folder_remains_a_noop`, `pasting_onto_itself_with_replace_is_a_noop` and the engine's into-itself guard. |
| `conflicts::transfer_noops_resolve_aliased_destinations_but_not_symlink_sources` | Same helper. |
| `conflicts::each_transfer_item_keeps_its_own_conflict_decision` | **Superseded.** It asserts legacy's batch abort (the first failing item stopped the paste). Jobs give every item its own outcome: the approved fix for the Replace-in-a-partial-clash bug. |
| `conflicts::keeping_both_preserves_transfer_noops` | **Superseded.** Legacy silently did nothing when a folder was pasted into itself; the engine refuses it with an error ("Can't put a folder inside itself"). |
| `conflicts::the_prompt_sees_the_name_a_paste_gives_each_item_and_what_has_it` | Paste planning for the conflict prompt. |
| `placement::a_case_only_rename_is_not_refused_as_taken_where_each_spelling_has_its_own_number` | Typed-name check for rename (`typed_names::rename_taken`); returns with rename. |
| `placement::a_hard_link_is_never_taken_for_a_case_alias` | Same. |
| `moves::moving_a_directory_falls_back_to_a_safe_copy_when_the_move_needs_one` | Legacy's injectable move attempt (`MoveAttempt`), an orchestration seam. `ops::move_as` renames and copies where no rename can place the item (cross-device or unsupported); `items_no_step_can_place_are_moved_by_copying` covers it. |
| `moves::a_failed_move_attempt_is_returned_without_falling_back` | Same seam. |
| `moves::a_successful_move_attempt_is_used_without_falling_back_to_copy` | Same seam. |
| `moves::same_filesystem_rename_is_not_held_for_the_removable_flush` | Legacy's per-paste device flush (`owed.rs`), replaced by durable batches (A7). |
| `moves::same_filesystem_replacing_move_renames_without_waiting_for_the_flush` | Same. |
| `moves::a_move_through_gio_keeps_the_links_it_leaves_out_at_the_source` | GIO remote copy: the remote/GVfs adapter. |
| `moves::deleting_a_source_gio_moved_keeps_the_links_it_left_out` | Same. |
| `moves::moving_to_a_remote_target_keeps_a_source_entry_changed_during_the_copy` | Remote target: the remote adapter. |
| `moves::moving_from_a_share_to_a_remote_target_right_after_a_paste_there_deletes_its_sources` | Same. |
| `merge::a_paste_records_no_more_than_its_limit_across_its_merges` | Legacy's budget of placed snapshots for undo without a Trash; the undo journal records steps by identity, capped by `undo::MAX_STEPS`. |
| `merge::a_flushed_merge_move_keeps_sources_when_published_entries_disappear` | Within one mount a merging move renames entry by entry: nothing is copied, so there is no flush and no published copy to vanish. |
| `replacement::replacement_publication_preserves_concurrent_arrivals` | Legacy `replacement.rs` publication internals (`publish_staged_replacement`), superseded by `ops::replace`'s own staging, exchange and set-aside. |
| `replacement::replacement_publication_reports_a_stage_it_cannot_remove` | Same. |
| `replacement::replace_falls_back_to_aside_when_trash_lies` | GIO's `g_file_trash` reporting success without moving (GLib #4060); the native Trash moves by rename, which cannot. |
| `paths::validated_children_are_confined_to_native_and_uri_parents` | A typed name may not leave its folder; returns with rename/new folder. The URI half belongs to the remote adapter. |

## Gaps the port found, and what was done

- **Fixed:** a paste into the folder an item is in failed (its name is taken) where legacy made a numbered
  duplicate. `ops::copy` now duplicates there.
- **Fixed:** items of one paste whose names differ only in case (`Notes`, `notes`) on a case-insensitive drive: the
  later one was refused where legacy numbered it and reported an alias. `jobs` now does so (legacy
  `case_variants.rs`, ported), deciding before copying, so no source is read twice.
- **Added:** moves that keep both, replace or merge (`ops::move_keeping_both`, `ops::move_replacing` with an
  atomic swap on one filesystem, `merge::merge_moving`, and the matching job actions and undo steps), and a name
  per item (`jobs::Item::name`; `copy_as`, `replace_as`, `move_replacing_as`, `merge_as`) for a conflict
  prompt's choice. A given name must be one plain name.
- **Fixed:** moving an item to the folder it is in, or replacing an item with itself, is a no-op, as in legacy.
- **Fixed:** a rename or move of an SMB folder refused while a file below it waits for the client's deferred
  close is retried until it closes, and a file still open elsewhere is reported as such; the native Trash does
  the same, through a restored `trash` hook in the engine's filesystem seam.
- **Fixed:** every system error had the kind `Failed`; errors now carry the kind their errno names (not found,
  permission denied, no space, busy, …), as legacy got from GIO. Placement errors too.
- **Fixed:** a move whose copy landed but whose source deletion stopped lost what it had kept; `Moved::stopped`
  and `Changes::kept` carry it, and a failed job item now reports its changes (`Outcome::Failed { error,
  changes }`, `kept` on `job.itemDone`). Before, sources a move left in place were never reported at all.
- **Fixed:** a merging move to another drive is now the engine's tree merge with proven deletion, as in legacy,
  so entries changed or added meanwhile stay; an original a Replace sent to the Trash comes back if its
  replacement was left out (a link FAT cannot hold, say), which would otherwise have lost it.
- **Fixed:** a move no rename can place on its own drive is copied, as legacy did; a failed final sync says that
  writes may still be pending.
- **Added:** `ops::rename`, which reports a reason as a rename prompt shows it.
- **Changed, to legacy's behaviour:** on a drive with no Trash, Replace deletes the original once its replacement
  is confirmed on the drive (`Replaced::original_deleted`, and no undo entry, since undo cannot bring it back);
  A3 refused instead. A Trash that exists but cannot be used still refuses. If the sync before the deletion
  fails, the replacement stays and the original is kept aside, named in the error.
- **Fixed:** a replacing move on one drive is now the item itself trading places with the original (or the
  original set aside), with no copy; across drives it copies into a stage, publishes it, and deletes the source
  only after, as legacy did; the version in between deleted the source first.
- **Fixed, safety:** a replacing move that set the original aside published whatever was at the source path,
  even a file swapped in meanwhile; it is now placed by identity. After an exchange, the name must hold the
  item itself, and an undo that cannot trade back puts the original straight back or says where it is.
- **Fixed, safety:** Replace compared the original by inode only, so on SMB, once server inode numbers went, a
  lookalike swapped in was replaced; it is now proven as deletion proves an item (identity traits, size,
  times), and the mount-aware comparison applies to the set-aside step too.
- **Fixed:** a replace now refuses before copying anything where neither strategy is safe; a cancel as the copy
  finished is honoured; a staged copy left by a failure is removed; a merge on a drive without a Trash sets
  clashing originals aside and deletes them once replaced, or puts them back.
- **Fixed, data loss:** a merge under a given name looked for clashes in the folder of the source's own name.
  With the source beside the target that was the source itself, so Replace sent the source's own files to the
  Trash; elsewhere it could replace in an unrelated folder of that name. Clashes are now found in the folder
  merged into, and merging a folder into itself is refused.
- **Changed, to legacy's behaviour:** a copy merge, and a merging move across drives, are the engine's own tree
  merge (each entry staged and whole, hard links kept as links, a target folder swapped for a link not
  followed, a taken name keeping both), not a walk of single copies; a merging move within one mount still
  renames entry by entry. Like legacy's, the tree merge stops at an entry it cannot read, keeping what it placed,
  which undo then takes away.
- **Fixed:** a hard link under another name is replaced like any item; only the same name in the same folder
  (differing at most in case) is the item itself.
- **Open:** a bulk path for many small items; `rename` on the protocol, new folder and new file with typed-name
  validation; a paste-planning call.

## Archives (legacy: 225 tests)

Out of scope for Phase A.
