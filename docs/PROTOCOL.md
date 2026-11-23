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
| `jobs.start` | `into`, `names?: "fix"\|"keep"` (fix), `label?` (what undo calls it; default from the actions, e.g. "Move 2 items"), `durable?` (sync the drive before finishing; default: when it is removable), `items: [{source, action: "copy"\|"merge"\|"move"\|"replace"\|"trash", conflict?: "fail"\|"keepBoth"\|"merge", choices?, otherwise?}]` | `{job}` |
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
exactly one `job.itemDone`; its outcome is `{status: "done", path, renamed: [{requested, savedAs, cause}],
skipped: [{path, kind}]}`, `{status: "failed", error}` or `{status: "notAttempted"}`.

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
| `trash.restore` | `info` (an entry's `.trashinfo` path from `trash.list`) | `{path}` |
| `trash.delete` | `info` | `null` |
| `trash.empty` | – | `{deleted, failed}` |
