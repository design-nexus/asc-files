# Porting ledger

The legacy app's behavioural tests (`adapters/local_operations/tests/**` at `nyx-legacy/nyx-files@36887fe`, 492
tests in 22 files plus the `fs_quirks` harness) move here file by file in Phase A8. They live in
`crates/asc-files-engine/src/adapters/local_operations/tests/`, driven by `tests.rs`, whose stand-ins keep the
legacy helpers' names: GLib futures became the engine's synchronous core, and legacy's paste through its
provider runs as a batch of `jobs` (`run_paste`), so these tests check the job runner itself.

Every legacy test is either ported and passing, ported and `#[ignore]`d with a reason, or listed below as not
ported with a reason. This list must be empty, or every entry justified, before Phase A ends. As of A8's end every legacy test is accounted for.

## Progress

| File | Legacy tests | Ported, passing | Ignored | Not ported |
|---|---|---|---|---|
| `aliases.rs` | 11 | 8 | 0 | 3 |
| `closes.rs` | 1 | 0 | 0 | 1 |
| `conflicts.rs` | 20 | 15 | 0 | 5 |
| `copy.rs` | 52 | 36 | 1 | 15 |
| `create_entry.rs` | 6 | 5 | 0 | 1 |
| `deletion.rs` | 30 | 21 | 0 | 9 |
| `file_copy.rs` | 29 | 28 | 1 | 0 |
| `folder_copy.rs` | 22 | 21 | 1 (upstream: run by another test) | 0 |
| `leftovers.rs` | 10 | 10 | 0 | 0 |
| `moves.rs` | 51 | 42 | 0 | 9 |
| `naming.rs` | 21 | 19 | 0 | 2 |
| `paste_results.rs` | 8 | 6 | 0 | 2 |
| `paths.rs` | 3 | 2 | 0 | 1 |
| `placement.rs` | 39 | 37 | 0 | 2 |
| `progress.rs` | 15 | 8 | 0 | 7 |
| `release.rs` | 5 | 0 | 0 | 5 |
| `replacement.rs` | 28 | 25 | 0 | 3 |
| `restore_safety.rs` | 28 | 28 | 0 | 0 |
| `sync.rs` | 23 | 6 | 0 | 17 |
| `undo.rs` | 23 | 16 | 0 | 7 |
| `trash_capabilities.rs` | 1 | 0 | 0 | 1 (ignored upstream too) |
| `merge.rs` | 66 | 37 | 1 | 28 |
| **All** | **492** | **370** | **4** | **118** |

## Ignored

| Test | Why |
|---|---|
| `merge::fat_merge_overwrites_the_names_it_plans_and_keeps_the_names_that_need_no_fix` | **Gap.** A merge finds clashes by source name, not by the name the destination stores: on FAT, `nested?` never meets `nested_`, so every entry is kept as both, even one whose exact name the user chose to replace. Nothing is lost. Legacy planned by stored names (`plan_names`), names needing no fix first; it returns with the paste-planning call. |
| `copy::a_paste_fixes_the_names_it_brings_onto_fat_and_keeps_those_already_there` | **Gap**, the planning one: a paste's names are not planned together, so an item needing no fix (`a_b`) does not keep its name over an earlier one fixed to it (`a:b`), and fails as taken. Returns with the paste-planning call. |
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
| `replacement::a_failed_publication_puts_the_original_back`, the FAT row | Re-aimed at the engine's publish, its fourth rename at the name on FAT as in legacy (the refused swap through an originals folder makes two). |
| `deletion::a_cancelled_delete_counts_only_what_it_left_whole_as_not_attempted`, the Trash row | It cancelled inside GIO's trash, which checked its cancellable just before renaming, so the item stayed. The native Trash moves in one rename and the hook fires at it, so the cancel lands as the item moves. A job still stops before its next item; the permanent rows run. |
| `deletion::trash_reports_the_item_it_moved_when_another_took_the_name_first`, the OpenAt row | Legacy swapped the item at the open it made before trashing; the native Trash makes none, so what holds the name at the rename goes. The rows swapping at the rename run. |
| Same test, the nfs3 row | Legacy reported the item it had checked: it read the item at its name through NFS's attribute cache, which still showed it after another was renamed over. The Trash moved the arrival, and the engine reports what reached the Trash. |
| `deletion::a_folder_only_leftovers_keep_goes_once_they_do_within_the_mount_s_wait`, the rows held by Files itself | Legacy knew what Files held (a preview's read, another of its operations) through `file_activity` and said so. Previews are the Qt app's (Phase B); the rows run held by a program or by nobody named. |
| `restore_safety::restore_leaves_in_the_trash_what_it_cannot_copy_or_finds_changed`, the merge-undo row | It also ran through legacy's merge undo, which restored a staged original; the engine's merge undo is its journal's, ported with `undo`. |
| `restore_safety::home_trash_fallback_finds_broken_symlinks_the_virtual_backend_has_not_refreshed`, the cancelled half | Legacy read records itself where GVfs's `trash://` listing lagged, and stopped at a cancel; the engine always reads them (`trash::entry_of`), and a lookup takes no cancel. That a broken link in the Trash is found with its record still runs. |
| `restore_safety::trash_items_whose_files_share_names_are_restored_whole`, the order of copies and deletions | Legacy copied both items before deleting either Trash copy, and checked that order. Jobs restore item by item; the proven deletion of one copy leaves the item sharing its file whole, which the rest checks. |
| `undo::undo_says_nothing_of_fixed_entries_gone_from_the_names_they_were_saved_as`, the row through GIO | Legacy also ran it with the original as a share's location, which GIO renamed: the remote adapter. The local row runs. |
| `undo::*`, requests with names `Fix` | Legacy's undo of a move took a name mode; one that fixed names was a move back as redo makes it. The stand-in runs those as the journal's redo does (`ops::move_as`, fixing), and the rest through the journal's undo. |
| `merge::*`, legacy's `MergeHooks` | Legacy injected its copy and its staging of originals; the ported tests run the engine's own merge, and an original staged by a hook is checked in the (isolated) Trash, or aside without one. Legacy's second plan, of placed snapshots for its own undo, is the journal's steps. |
| `merge::a_merge_replaces_nothing_with_a_link_the_destination_cannot_hold`, the FAT row | Run without a Trash: a simulated FAT drive's Trash is the home one, across the simulated mount, where a real FAT drive has its own. |
| `merge::merging_recurses_into_subdirectories_shared_by_both_sides`, the listing | Legacy listed in chunks; the engine's copy lists a folder whole. Either goes through the filesystem seam, which is what is checked. |
| `merge::merge_without_trash_sets_originals_aside_and_removes_them_after_success`, the GIO row | Legacy also ran a GIO Trash that reported failure without a reason (`FalseWithoutError`), which it took for no Trash. The native Trash's failure is an I/O error, which stops the merge. Its merging moves within one drive set originals aside through the replace, in an originals folder, where legacy used the merge's. |
| `merge::a_failed_merge_without_trash_puts_back_what_it_can_and_names_the_rest` | Legacy injected a copy that wrote part of one file and failed, and a cancelled one; here one item is copied, one is a socket the copy leaves out, and the drive cannot confirm the writes, which puts the first original aside and the second back. The cancelled row checked legacy's wording for a cancel. |
| `merge::a_merge_without_trash_reports_its_folder_kept_by_an_original_still_open`, the order of what is kept | The engine reports what it left out as it goes; the items are compared in any order. |
| `merge::merge_originals_are_deleted_only_once_the_merge_is_flushed`, the source on a failed flush | Legacy copied a merging move within one drive, so a failed flush kept the source as well as the original. The engine renames there: the moved item is at the name, and the original stays aside, named in the error. |
| `merge::a_failed_merge_preserves_the_destinations_existing_contents`, `merge::a_staging_failure_aborts_the_merge_and_reports_only_what_was_staged` | Legacy's copy and staging hooks became a cancel and a refused Trash through the filesystem seam. Legacy left an original staged before the failure; the engine puts it back, which is checked. |
| `merge::*`, the GIO copy rows | Legacy ran several merge tests with GIO's copy as well as its own: the remote adapter. |
| `merge::*` undo tests | Legacy undid a merge from lists of what it created and overwrote, finding originals through a lookup hook; the ported tests run a real merge, file its steps in a journal of their own as a job does, and undo through the journal. The bookmark half of `merge_undo_restores_staged_originals_and_removes_created` is the app's. |
| `merge::a_merge_without_trash_cancelled_after_it_placed_a_copy_puts_the_original_back` | Legacy discarded the copy it had placed and put that original back; the engine keeps what it placed, which undo takes away, and that original waits aside, named in the error. The other original, whose name was left free, is back. |
| `copy::*`, the GIO rows of the naming tables | Legacy ran its naming tests with GIO's copy as well: the remote adapter. |
| `copy::a_refused_name_takes_the_first_stricter_rules_it_passes_and_teaches_only_a_mount_that_learns`, the refusal counts of mounts that learn | Legacy planned a paste whole, so names planned before a mount learned were each refused once; the engine learns as it goes and refuses fewer. The names saved are checked for every mount, the counts for the one that never learns. |
| `leftovers::stale_probes_go_only_when_empty_and_old`, the mount-lane half | Legacy swept on its mount's background lane and checked that a slow sweep kept the lane answering. The engine has no lanes: the backend runs the sweep on a thread of its own. That the sweep removes a listed stale probe is still checked. |
| `paste_results::an_item_replaced_without_trash_is_left_out_of_the_paste_undo`, the order of moves | Legacy moved the items without a conflict first; jobs keep the order asked for. The moves are compared as a set. |

## Not ported

| Test | Why |
|---|---|
| `aliases::identical_top_level_names_from_different_folders_are_numbered_and_reported` | Paste planning (`transfer_targets`): which items of one paste meet each other's names was decided before copying. Now the client chooses Keep both per item. Returns with a planning call for the Phase B conflict prompt. |
| `aliases::an_item_from_another_folder_meets_the_original_of_a_duplicate` | Same. |
| `aliases::same_named_items_from_three_folders_all_land_whatever_their_order_and_choice` | Same. |
| `naming::pasted_names_fixed_for_fat_are_planned_after_the_names_kept` | Same: the names a conflict prompt shows. |
| `naming::transfer_targets_resolve_trash_leaf_names` | `trash://` URIs belong to the remote/GVfs adapter. |
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
| `deletion::cancelling_between_deletions_reports_completed_and_unattempted_items` | Legacy ran in lock step, so a cancel sent on the first item's progress landed before the second began. A job runs ahead of its events, so the second may be under way or done. That a cancel leaves the rest not attempted is `tests/jobs.rs`'s `cancelling_reports_the_rest_as_not_attempted`. |
| `deletion::provider_cleans_only_confirmed_pins_on_failure_and_cancellation` | Sidebar bookmarks (pins) of deleted items: the app (Phase B). |
| `deletion::deleting_browser_can_drop_after_a_confirmed_deletion_without_losing_pin_cleanup` | Same, through the GTK `Browser`. |
| `deletion::bookmark_cleanup_failure_preserves_the_successful_deletion_outcome` | Same. |
| `deletion::deletion_error_summaries_are_bounded_and_report_the_failure_count` | The dialog's summary of failures. The protocol reports each item's error; the client words the summary. |
| `deletion::deletion_failures_explain_trash_problems_and_keep_other_details` | GIO error codes worded for the dialog. Native Trash errors carry their kind (`Unsupported` tells a client to offer Delete); the wording is the client's. |
| `deletion::a_refused_trash_is_retryable_and_remembered_only_on_fuse` | GIO's `trash_support` remembering FUSE mounts that refused `g_file_trash`; the native Trash decides by the drive's Trash folder. |
| `deletion::verified_trash_learns_fuse_refusals_and_cleans_leftovers_on_both_worker_routes` | Same, and GIO's half-written `.trashinfo` leftovers. |
| `deletion::a_trash_that_moves_nothing_is_retryable_and_records_no_undo` | `g_file_trash` reporting success without moving (GLib #4060); the native Trash moves by rename. |
| `undo::a_confirmed_undo_conflict_replaces_the_newer_item` | **Gap.** The journal's undo never replaces an item that took the name since; legacy, after the user confirmed, did. Returns with the conflict prompt (Phase B). |
| `undo::undoing_a_grouping_reports_the_trashed_group_folder` | Grouping (moving items into a new folder) is an app command with no engine counterpart yet; its undo returns with it. |
| `undo::undoing_a_grouping_without_trash_removes_only_an_empty_group_folder` | Same. |
| `undo::a_typed_name_at_a_gvfs_address_is_refused_by_the_rules_of_its_scheme` | A GVfs address: the remote adapter. |
| `undo::a_rename_gio_has_no_backend_for_fails_instead_of_ending_the_process` | GIO: the remote adapter. |
| `undo::undoing_a_move_back_to_another_kind_of_location_gives_fixed_names_back` | Its premise is an original addressed through GIO, which renamed with its own rules: the remote adapter. The local give-back of such names is `undoing_a_move_gives_fixed_names_back_and_reports_those_it_cannot`. |
| `undo::undoing_a_cross_volume_move_deletes_the_moved_items_once_they_are_flushed` | Legacy flushed once for a whole undo and deleted every moved item after. The journal undoes item by item, each item's source deleted only after its own copy is flushed, so a failed flush keeps that item whole and leaves the ones before it done. |
| `merge::a_merge_onto_fat_takes_the_rules_of_a_folder_inside_it_only_from_another_device_mounted_there` | Its FAT row replaces an original by the name FAT stores, the planning gap above; it returns with it. |
| `merge::undoing_a_merge_without_trash_opens_only_its_own_read_only_folders_to_remove_them` | **Gap.** On a drive without a Trash, undo deletes only copies of files that are unchanged; a folder an operation created is left, so undoing a folder copy to a FAT stick leaves the folder. Legacy removed a created tree whose every entry was as placed, opening its own read-only folders to do so. |
| `merge::a_created_folder_changed_while_undo_deletes_it_fails_as_not_empty` | Same gap: without a Trash, the engine deletes a created folder only while it is empty and unchanged (since A9), never a tree. |
| `merge::what_an_operation_placed_is_read_back_only_until_its_tick_passes`, `merge::an_item_put_at_a_placed_name_right_after_placing_stays_there`, `merge::a_merge_undo_compares_what_it_placed_as_a_move_compares_its_sources`, `merge::a_merge_undo_restores_an_original_only_over_a_path_it_checked`, `merge::an_undo_keeps_what_a_cancel_stopped_the_operation_from_recording`, `merge::a_merge_records_what_it_creates_only_where_undo_cannot_trash_it`, `merge::undoing_a_replace_deletes_what_it_placed_only_if_all_of_it_is_unchanged` | Legacy's placed snapshots: what an operation placed, read back within a clock tick and compared at undo. The journal records each step's identity and undoes it only if the item is the same one and unchanged (`a_merge_undo_deletes_only_what_the_merge_left_unchanged`). |
| `merge::an_undo_keeps_what_was_too_big_to_record`, `merge::a_created_tree_too_big_to_record_leaves_its_entries_to_the_rest` | Legacy's budget of snapshots; the journal caps steps (`undo::MAX_STEPS`) and files a larger entry as too large to undo. |
| `merge::a_merge_undo_without_trash_deletes_only_what_the_merge_created_unchanged`, `merge::a_merge_undo_without_trash_deletes_what_its_paste_copied_only_if_unchanged` | Covered by the Trash-less row of `a_merge_undo_deletes_only_what_the_merge_left_unchanged`. |
| `merge::merge_undo_uses_recorded_identity_when_trash_dates_collide`, `merge::a_trashed_original_is_passed_over_only_where_its_mount_proves_another`, `merge::undo_finds_an_original_in_the_trash_of_its_own_mount` | Legacy found an original in the Trash by path, date and mount; the journal records its Trash record and identity (`an_undo_never_brings_back_another_item_trashed_later_from_the_same_path`). |
| `merge::a_merge_undo_cut_off_counts_an_item_as_not_attempted_only_when_it_left_it_whole`, `merge::a_kept_item_counts_as_failed_when_a_later_item_is_cut_off`, `merge::a_merge_undo_never_counts_an_item_it_could_not_check_as_undone`, `merge::a_merge_undo_cancelled_while_it_restores_an_original_ends_as_a_cancel`, `merge::a_merge_undo_cancelled_while_it_compares_an_item_leaves_it_whole`, `merge::a_merge_a_cancel_cuts_off_counts_as_failed_as_it_keeps_what_it_placed` | How legacy's undo and paste events counted items a cancel cut off. The journal counts a step done only once it is, and lists every other with why, a cancel included (`a_cancelled_merge_undo_reports_everything_not_attempted`). |
| `merge::a_merge_from_a_location_without_paths_does_not_claim_provenance_for_undo`, `merge::a_merge_from_a_location_without_paths_keeps_unproven_copies_during_no_trash_undo` | GIO locations without paths: the remote adapter. |
| `merge::a_merge_undo_drops_from_view_only_what_it_removed` | The app's view (Phase B). |
| `progress::completed_gio_result_wins_a_cancellation_race`, `progress::local_task_batches_wait_for_every_started_task_after_an_operation_error` | GLib task helpers: the engine has neither. |
| `progress::transfer_progress_aggregates_file_bytes_without_emitting_per_file`, `progress::copy_byte_progress_crosses_worker_threads` | Legacy's `TransferProgressTracker`; the engine counts in shared atomics (`ops::Progress`) that jobs report as `job.progress`, which the other progress tests check. |
| `progress::fat32_copy_errors_without_english_size_text_skip_an_uncancellable_flush` | Legacy read GIO's error text to tell FAT32's size refusal from others; the engine refuses an oversized file before copying (`fat32_size_check_rejects_oversized_nested_file_before_copy`). |
| `progress::bulk_copy_reports_bytes_before_each_file_completes` | Legacy's bulk path for many small items, the gap above. |
| `progress::cancelling_between_moves_reports_completed_and_unattempted_sources` | Lock step, as `deletion::cancelling_between_deletions_reports_completed_and_unattempted_items`; covered by `tests/jobs.rs`. |
| `sync::owed_deletions_close_in_batches`, `sync::a_failed_barrier_deletes_nothing_in_its_batch_and_stops_the_move`, `sync::cancelling_a_move_closes_its_pending_batch`, `sync::many_moved_files_share_a_barrier_and_are_copied_in_parallel_unless_a_disk_spins`, `sync::names_of_one_file_moved_as_separate_items_are_all_deleted`, `sync::a_writeback_lost_during_a_move_keeps_every_source_it_may_hold`, `sync::a_batch_closes_once_its_first_entry_is_five_seconds_old`, `sync::a_failed_barrier_stops_a_bulk_move`, `sync::bytes_already_written_are_flushed_before_transfer_failure` | Legacy's `owed` batches: the sources a paste's moves copied were deleted together, a batch at a time, after one shared flush. The engine moves item by item, each source deleted only after its own copy's flush (`the_barrier_before_deleting_a_moved_source_follows_the_destination_family`), so a failed flush keeps that source and stops nothing else. Sharing one flush across many small moves is part of the bulk-path gap. |
| `sync::removable_copy_roots_are_synced_once`, `sync::sync_filesystem_on_a_local_directory_succeeds`, `sync::failed_flush_after_cancel_reports_that_writes_may_still_be_pending`, `sync::fat32_limit_failure_does_not_start_an_uncancellable_device_flush` | Legacy's paste helpers (`removable_sync_roots`, `sync_filesystem`, `stop_transfer`). A durable job opens one flush of its drive before it writes and reaches it at the end, whatever happened (`failed_final_flush_reports_that_writes_may_still_be_pending`). |
| `sync::bytes_already_written_are_flushed_before_cancellation_and_cancel_is_not_pasted`, `sync::a_cancel_after_the_flush_starts_does_not_report_success` | Cancels sent from inside the paste's events, in lock step; a durable job reaches its flush after a cancel too, and reports the cancel. |
| `sync::files_gio_wrote_are_synced_one_by_one_where_the_barrier_misses_them` | GIO's copy: the remote adapter. |
| `copy::cancelling_staged_remote_file_copy_removes_only_the_incomplete_stage`, `copy::staged_remote_file_copy_preserves_a_racing_destination`, `copy::a_remote_keep_both_copy_takes_the_next_name_when_one_is_taken_meanwhile`, `copy::a_remote_keep_both_copy_numbers_a_long_name_within_the_limit`, `copy::a_gio_copy_lists_a_whole_folder_first_only_where_the_mount_takes_two_names_for_one`, `copy::a_gio_copy_that_fails_reports_the_fixed_name_of_a_folder_it_made` | Copies from remote locations through GIO: the remote adapter. |
| `copy::staged_publication_without_rename_flags_places_the_stage_itself`, `copy::staged_publication_refuses_to_overwrite_an_existing_target`, `copy::staged_publication_preserves_racing_destinations`, `copy::staged_symlinks_are_recreated_where_links_are_missing`, `copy::staged_fallback_removes_reserved_directory_on_copy_error`, `copy::a_stage_left_after_the_exclusive_copy_is_reported`, `copy::a_stage_swapped_before_publication_is_neither_published_nor_deleted`, `copy::cancelling_staged_fallback_removes_its_reserved_directory` | Legacy's `publish_staged_without_replace`, the glue that published a sibling stage its GIO copies made. The engine publishes inside its own copy; that publication's races, refusals and clean-up are the ported `placement` and `file_copy` tests. |
| `copy::failed_incomplete_copy_cleanup_is_reported_as_a_failure` | Legacy's wording helper `copy_failure_after_cleanup`; the engine says the incomplete copy could not be removed (seen in `restore_safety::a_restore_cancelled_while_it_copies_an_item_ends_as_a_cancel`). |
| `create_entry::a_folder_reported_by_a_file_create_is_a_taken_name` | Legacy's private helper `name_taken`; its rule (a folder at the name refuses an exclusive file create with `EISDIR`, so the name is taken) is in `ops::create` and runs in the ported concurrency tests. |
| `paste_results::a_cancel_that_cuts_off_an_item_counts_it_as_not_attempted` | Drives the GTK app's `Browser`. Cancelling is the job runner's (`tests/jobs.rs`: items not begun are `NotAttempted`). |
| `paste_results::only_a_cut_off_between_local_locations_counts_as_not_attempted` | Legacy's rule for remote (`sftp://`) ends of a cut-off paste: the remote adapter. |
| `release::*` (5) | Files letting go of its own previews of an item before changing it: the app's preview layer (Phase B). |
| `trash_capabilities::isolated_trash_supports_read_copy_move_restore_and_delete` | Ignored upstream; GVfs `trash://` end to end: the remote/GVfs adapter. |
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
- **Added:** typed-name validation (legacy `typed_names.rs`, ported) and `ops::create` for a new folder or file:
  refused before anything is made where the drive would store another name, an existing name refused or, when
  asked, the next free number taken, safe against creates racing for the same name.
- **Fixed:** on a drive with no Trash, Replace swaps the original out into an originals folder under its own
  name, as legacy did, rather than leaving it at the stage's hidden name; an original that then cannot be
  deleted (undeletable, or still open in a program on NFS or SMB) stays there and is reported as kept
  (`KeptCause::OriginalNotRemoved`, with why), where before it was deleted silently or lost under the stage name.
  `jobs` carries `original_deleted` (`originalDeleted` on `job.itemDone`), and a replace whose original was
  deleted or kept is neither filed for undo nor reported as created.
- **Added:** recovery of what an interrupted operation left (legacy `leftovers.rs`, ported): `recovery::recover`
  puts set-aside originals back under their names, numbered where a name is taken now, never over an item;
  `recovery::review` says what each leftover holds; `recovery::sweep_probes` removes stale probes. On the
  protocol as `leftovers.recover`, `leftovers.review` and `leftovers.sweep`. Before, the engine left these
  folders for good.
- **Fixed:** a replace staged its copy under the `.asc-files-replacement-` prefix, which marks a possible
  original, so a copy an unplug cut off would have been shown as one; it now stages under `.asc-files-copy-`,
  an incomplete copy, as legacy did.
- **Added:** permanent delete (`ops::delete`, `jobs::Action::Delete`, `delete` on the protocol): the engine's pinned,
  parallel walk was there but nothing reached it.
- **Fixed:** each item of a batch waited the whole budget for files still open on NFS or SMB, so a delete or Trash
  of N folders held open waited N times as long; one wait now covers the batch, as legacy's did.
- **Fixed:** restoring from the Trash was a single no-replace rename: it failed outright where a drive has no such
  rename (FAT, many FUSE and network drives) or none for folders, took no pinned folder (a symlink swapped into the
  original's path could lead it elsewhere), and never checked the item was still the one read. It is now legacy's
  restore (`ops::put_back`, ported): placed by identity, the original's folder opened beneath the drive's root,
  copied where nothing can place it and the Trash copy deleted only once the copy is on the drive, with what stays
  in the Trash reported. It refuses an original off the item's drive, across a bind mount or subvolume, or inside
  a Trash, and a destination the user confirmed that the record no longer names.
- **Added:** `jobs::Action::Restore` (`restore` on the protocol, with `to`, the confirmed destination) and
  `trash::entry_of` for an item a Trash folder's listing shows; `trash.restore` now answers from its own thread.
- **Fixed:** a rename moved the item, so a case-only rename on a drive that ignores case was refused as taken;
  `ops::rename` now renames in place as legacy did (set aside and placed back under the new spelling), with
  legacy's wording, saying where an item was left if the way back failed. `ops::rename_typed` checks a typed name
  first.
- **Fixed:** an undone move gave names back with a raw rename that bypassed the engine's filesystem layer, and
  reported every name it could not give back, and every entry gone since, as a failure. It now renames in place
  through the layer; an entry gone since is passed over, and one keeping its saved name is reported as renamed (name
  taken, or refused) or kept, as legacy did (`undo::Outcome::changes`, `renamed` and `kept` on the protocol).
- **Fixed, data loss:** a merge that had to put an original back (its incoming item left out, as a link FAT cannot
  hold) ignored a failure to do so: the original stayed in the Trash with nothing said, or, without a Trash, the
  folder it waited in was then deleted with it. A failure is now reported with where the original is, and that
  folder stays. Originals are set aside and put back through the engine's filesystem layer, and if setting one
  aside fails, every original set aside so far goes back before the merge stops.
  (`an_original_that_cannot_go_back_stays_aside_and_is_named`, new.)
- **Ported:** legacy's `merge_aside`, the merge's set-aside where the drive has no Trash: one hidden folder in the
  target, held open by identity, each original placed into it by identity (refused across a nested mount), put
  back where the merge left its name free, and deleted only once the merge is whole and on the drive, waiting for
  a file still open on a share and reporting it. A merge that stops puts back what it can and names the folder
  holding the rest. It replaces the engine's plain renames into a folder.
- **Fixed, data loss:** a copy merge on a drive without a Trash filed the copy over a replaced original as a plain
  copy. The original was deleted once the merge was on the drive, so undo would then have taken the copy away
  with nothing to put back. Such a copy is no longer filed, as legacy left it out.
- **Fixed:** once a merge without a Trash was on the drive, its aside folder was deleted by one walk, which
  stops at its first refusal. A file there still open on a share then hid the rest, and the folder was reported
  as not removable rather than held open. What else can go now goes entry by entry first. A merging move no
  longer needs a second flush, its copies having reached the drive before any source went.
- **Fixed:** an item a merge copied under a name the copy changed (cut to fit, fixed, or numbered as it met
  another) was not filed, so undo would have left it behind. It is now filed from the copy's own record of the
  names it changed.
- **Changed, to legacy's behaviour:** undoing a copy that is gone already counts as done instead of being reported
  as left.
- **Fixed:** undoing a copy with a Trash checked only that it was still the same item, so a copy the user had
  edited since went to the Trash. A copy changed since now stays in place and is reported, as legacy did.
- **Added:** a batch measures its items before writing (`ops::measure`), and `job.started` says how much it will
  write (`totalBytes`, `totalFiles`). An item holding a file too large for its destination (FAT32's 4 GiB) fails at
  once, naming the file, where before the copy ran until the drive refused it at 4 GiB.
- **Fixed:** a replace on a drive without a Trash deletes the original once the copy is on the drive, but the copy
  was not synced file by file where the drive's flush does not cover it (SMB); now it is. A barrier before a
  moved source is deleted syncs the folder too on a removable drive, as legacy's did.
- **Added:** `job.flushing` while a durable batch waits for its drive, and `flushes` in `job.progress` for the
  flushes an item began (before deleting a moved source, say), so a client can say the drive is still being
  written to; legacy's `FlushingToDevice`. A failed final flush says that writes may still be pending and to wait
  for safe eject. An item's last progress is reported before it ends.
- **Fixed:** an item's own name did not take its destination folder's rules: on a GVfs bridge backend (a phone over
  MTP) or a mount that had learned from refusals, a name inside a copied folder was fixed but the item's own was
  tried as it was. It is now fixed first, as legacy did, and a fixed name that is taken is numbered, as no one
  chose what should happen to it.
- **Fixed:** a link the destination cannot hold, selected itself rather than found inside a folder, failed with an
  unexplained system error; it is now left out and reported (kept at its source by a move), and the paste goes
  on, as inside a folder (`Changes::left_out`, `leftOut` on `job.itemDone`). Nothing is filed for undo for it.
- **Open:** undo of created folders without a Trash (see above).
- **Added (A9):** `ops.rename` and `ops.create` on the protocol, filed for undo (`undo::Step::Created`, and a
  rename as a move within its folder, undone and redone as a rename so a change of case alone works). Undo
  without a Trash now also deletes an empty folder an operation made, while it is unchanged.
- **Open:** a bulk path for many small items; a paste-planning call.

## Archives (legacy: 225 tests)

Out of scope for Phase A.
