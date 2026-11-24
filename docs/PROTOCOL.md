# Backend protocol (version 1)

`asc-files-backend` speaks JSON-RPC 2.0 on stdin and stdout, one message per line. stdout carries only protocol
lines; diagnostics go to stderr. Replies may arrive out of order (a listing answers on its own thread), so match
them by `id`. Paths are UTF-8 strings in version 1; byte-safe names come with version 2 (A9).

Errors: `-32700` parse, `-32600` invalid request (including anything before `initialize`), `-32601` unknown
method, `-32602` invalid params (including an unknown listing), `1` the operation was refused. A code-1 error's
`message` is JSON: `{"kind": "<ErrorKind>", "message": "<sentence for the user>"}`.

## Session

| Method | Params | Result |
|---|---|---|
| `initialize` | – | `{protocolVersion, server, capabilities}` |
| `shutdown` | – | `null`; running jobs are cancelled, then the process exits |

## Listings

A listing reads its folder once, sorts it, and serves rows in windows; each row is statted when its window is
asked for. Every read is a new **generation**: when a reply's or notice's generation differs from the one a view's
rows came from, the view asks for its windows again.

| Method | Params | Result |
|---|---|---|
| `list.open` | `path`, `showHidden?` (false), `sort?: {key: "name"\|"size"\|"modified", descending?, foldersFirst?}` (name, ascending, folders first) | `{listing, generation, count, followsChanges}` |
| `list.window` | `listing, start, count` | `{listing, generation, start, rows}` |
| `list.reorder` | `listing`, and any of `showHidden`, `sort` (what is left out keeps its value) | `{generation, count}` |
| `list.refresh` | `listing` | `{generation, count}` |
| `list.close` | `listing` | `null` |

A row: `{name, kind: "folder"|"file"|"link"|"special", linkToFolder, hidden, size, modified, modifiedNs, mode}`;
an entry that went since the read is `{name, kind, linkToFolder, hidden, gone: true}`.

`followsChanges` is false when inotify refused the folder: the listing works but goes stale until refreshed.
A drive with two reads already outstanding (a stalled share) refuses `list.open` with kind `TimedOut`.

Notifications:
- `list.changed {listing, generation, count}`: the folder changed outside and was read again.
- `list.gone {listing, path, nearest}`: the folder was deleted or moved away; `nearest` is the closest folder
  left above it.
- `list.unmounted {listing, path, nearest}`: its drive was unmounted.

## Jobs

| Method | Params | Result |
|---|---|---|
| `jobs.start` | `into`, `names?: "fix"\|"keep"` (fix), `label?` (what undo calls it; default from the actions, e.g. "Move 2 items"), `durable?` (sync the drive before finishing; default: when it is removable), `items: [{source, action: "copy"\|"merge"\|"move"\|"replace"\|"trash"\|"delete"\|"restore", conflict?: "fail"\|"keepBoth"\|"merge", choices?, otherwise?}]` | `{job}` |
| `jobs.cancel` | `job` | `null` |
| `jobs.clashes` | `into`, `sources` | `{items: [{source, clashes: [{path, incoming, existing}]}]}`, each side `{isFolder, size, modified}`; or `{source, error}` |

A `merge` item merges a folder into the folder of its name in `into` (a plain copy where there is none). Folders
on both sides merge in turn; for any other clash, `choices` maps its path inside the merged folder (as
`jobs.clashes` gives it) to `"keepBoth"` (copied in as `name (1)`), `"replace"` (copied over it; the original goes
to the Trash once the copy holds the name) or `"skip"`; `otherwise` (default `"keepBoth"`) covers the rest. A copy
with `conflict: "merge"` is a merge keeping both everywhere. A merge goes on past an item that fails; its outcome is
then `failed`, with the first error, and what it did is still filed for undo.

Batches writing to the same drive run one after another, in the order they were started; a batch that has to
wait sends `job.queued {job, ahead}` before `job.started`. Batches for different drives run side by side.

`names: "fix"` saves a name the destination drive cannot store under the closest one it can; `"keep"` fails such
an item instead (meant for undo). Notifications: `job.started {job, total}`, `job.progress {job, item, bytes, files}`,
`job.itemDone {job, index, outcome}`, `job.finished {job, done, failed, notAttempted, cancelled, undo, durable, settled?}`, where `undo` is the batch's
history entry, or `null` when nothing it did can be undone (with `undoError` if the history could not be
written), and for a durable batch `settled` says whether the drive confirmed everything was written
(`settleError` if not): only then is a removable drive safe to unplug. Every item gets
exactly one `job.itemDone`; its outcome is `{status: "done", path, renamed, skipped, kept, originalDeleted}`,
`{status: "failed", error, renamed, skipped, kept, originalDeleted}` or `{status: "notAttempted"}`, where `renamed` is
`[{requested, savedAs, cause}]`, `skipped` `[{path, kind}]`, and `kept` `[{path, cause: {reason, …}}]`: source
entries a move left in place (changed after the copy read them, undeletable, still open elsewhere, …), and an
original a replace could not delete on a drive with no Trash, left in an originals folder beside the target.
`originalDeleted` is true when a replace deleted the original, its drive having no Trash: undo cannot bring it
back. A failed item still lists what it did before it stopped.

A `delete` item is removed for good, without the Trash, and is never filed for undo; a folder goes with
everything in it, links inside are removed rather than followed, and a folder renamed or swapped meanwhile is
never followed out of the tree. Like `trash`, it ignores `into`.

A `restore` item's `source` is an item in a Trash's `files` folder (the `path` a `trash` item reported, or
`file` from `trash.list`); it goes back to where its record says it came from, never over an item that took that
name, never off its drive or across a bind mount or subvolume boundary, and never inside a Trash. With `to`, the
destination the user confirmed, it is refused if the record now says otherwise. Where no rename can place it, it
is copied and the Trash copy deleted only once the copy is on the drive; entries left in the Trash (changed
meanwhile, or undeletable) are its `kept`. Not filed for undo.

Items may also take `name` (the name a conflict prompt offered; one plain name), and moves take `conflict`
`"fail"`, `"keepBoth"` or `"replace"`; a `merge` item with `move: true` merges by moving.

## Undo

One history per session, shared by every Files process (`$XDG_RUNTIME_DIR/asc-files/undo-journal`), 50 entries deep.
Every batch is filed when it finishes, even a cancelled or partly failed one. A merge is filed item by item:
what it copied in and what it replaced; the folder merged into is never undone.

| Method | Params | Result |
|---|---|---|
| `undo.history` | – | `{undo: {id, label} \| null, redo: {id, label} \| null}` |
| `undo.undo` | – | `{id, label, done, left: [{path, error}]}` |
| `undo.redo` | – | the same |

Undo reverses the newest entry, newest step first: a copy goes to the Trash (or, on a drive without one, an
unchanged file is deleted), a move goes back under its old name, a trashed item is restored, and a replace puts
the original back and the copy in the Trash. A step whose item changed since is left as it is and listed in
`left`; the rest still run. Redo does the entry again and never replaces what holds a name. Nothing to undo or
redo is a code-1 error of kind `NotFound`; without a session folder, kind `NotSupported`, and `initialize` does
not list the `undo` capability.

## Trash

| Method | Params | Result |
|---|---|---|
| `trash.list` | – | `{entries: [{info, file, original, deleted}]}` |
| `trash.restore` | `info` (an entry's `.trashinfo` path from `trash.list`) | `{path}`; answered from a thread of its own, as it may copy. A `restore` job reports progress and can be cancelled |
| `trash.delete` | `info` | `null` |
| `trash.empty` | – | `{deleted, failed}` |

## Leftovers

An operation cut off part way (a crash, an unplugged drive) can leave hidden entries beside its target:
`.asc-files-originals-*` and `.asc-files-merge-originals-*` folders holding originals a replace or merge set
aside, `.asc-files-copy-*` copies that never finished, and `.asc-files-probe-*` tests of what a drive can do.
A listing shows them like any hidden entry; these let a client say what one holds and put originals back.
Each runs on a thread of its own, since it may wait on a share.

| Method | Params | Result |
|---|---|---|
| `leftovers.review` | `folder`, `names` (leftovers listed there) | `{leftovers: [{name, modified, inside}]}`, `inside` `{kind: "item"}`, `{kind: "items", names, more}` (up to 1,000 names; a merge's by their path inside it), `{kind: "stillOpen", reason}`, `{kind: "gone"}` or `{kind: "unreadable", reason}` |
| `leftovers.recover` | `path` (an originals folder) | `{restored, renamed, kept, failed: [{path, error}], notAttempted, removed}` |
| `leftovers.sweep` | `folder`, `names` | `null` |

`leftovers.recover` puts each original back under its name beside the folder (a merge's into the folder it came
from), and never over an item that holds that name now: such an original comes back numbered, listed in
`renamed`. `removed` says the folder is gone, everything in it having been put back; `kept` lists what stays in
it and why (a file still open in a program, say). A path that is not an originals folder, or one holding only
another program's leftover, is refused. `leftovers.sweep` removes the probes among `names` that are empty and
more than an hour old, each tried once per backend.
