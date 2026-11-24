# Backend protocol (version 2)

`asc-files-backend` speaks JSON-RPC 2.0 on stdin and stdout, one message per line. stdout carries only protocol
lines; diagnostics go to stderr. Replies may arrive out of order (a listing answers on its own thread), so match
them by `id`. Every message, and each method's params and result, is described by
[`protocol.schema.json`](protocol.schema.json) (JSON Schema 2020-12); the backend's tests check every message they
exchange against it.

**Messages.** A request is `{"jsonrpc": "2.0", "id", "method", "params"?}`, with `id` a string, a number or null
and `params` an object; batches are not supported. A message without `id` is a notification: it is carried out
and never answered, not even with an error. Every request is checked in full before anything is done.

**Paths and names** are JSON strings when they are UTF-8. One that is not is sent as
`{"bytes": "<base64>", "display": "<the name with U+FFFD for what is not UTF-8>"}`; show `display`, and send the
object (or just `{"bytes": …}`) back to name the item. Paths sent to the backend must be absolute, and names
(`name`, `names`) plain: no `/`, not `.` or `..`. Names the user types (`ops.rename`, `ops.create`) are strings.

**Errors.** `-32700` parse; `-32600` invalid request (not JSON-RPC 2.0, or anything before `initialize`);
`-32601` unknown method; `-32602` invalid params, with the field named in the message (`items[2].source must be
an absolute path`), and an unknown listing; `1` the operation was refused: `message` is a sentence for the user
and `data.kind` one of the kinds below, for code to branch on. Errors inside results and notifications are
`{kind, message}` with the same kinds:

`failed`, `notFound`, `exists`, `isDirectory`, `notDirectory`, `notEmpty`, `notRegularFile`, `notSymbolicLink`,
`filenameTooLong`, `invalidFilename`, `tooManyLinks`, `noSpace`, `invalidArgument`, `permissionDenied`,
`notSupported`, `notMounted`, `cancelled`, `readOnly`, `timedOut`, `busy`, `wouldBlock`, `tooManyOpenFiles`,
`invalidData`, `brokenPipe`, `noSuchDevice`, `symlinkUnheld`.

**Changes from version 1.** Non-UTF-8 paths and names travel as bytes instead of being mangled; a refusal's kind
is in `data.kind` (camelCase), not JSON inside `message`; `jsonrpc: "2.0"` is required; relative paths and
non-plain names are refused; `ops.rename` and `ops.create` are new.

## Session

| Method | Params | Result |
|---|---|---|
| `initialize` | `protocolVersion?` (the version the client speaks; any other than 2 is refused, with `data.supported`) | `{protocolVersion: 2, server, capabilities}` |
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
A drive whose two read places are still taken after two seconds (a stalled share) refuses `list.open` with kind `TimedOut`; folders opened together on a drive that answers just queue.

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
| `jobs.plan` | `into`, `sources` | `{items: [{source, name, renamed, incoming, existing, same, aliasOf}]}`, or `{source, error}` |

`jobs.plan` says what pasting each source into `into` would meet, changing nothing: `name` is the name the
destination's drive would store it under (`renamed` when that is not its own: a FAT stick stores `a:b.txt` as
`a_b.txt`), `existing` what holds that name now (`null` if nothing), `same` when that is the source itself (a paste
into the folder it is in), and `aliasOf` the index of an earlier source landing on the same name. A conflict
prompt passes `name` on as the item's `name`, so Replace replaces the item that holds it.

A `merge` item merges a folder into the folder of its name in `into` (a plain copy where there is none). Folders
on both sides merge in turn; for any other clash, `choices` maps its path inside the merged folder (as
`jobs.clashes` gives it) to `"keepBoth"` (copied in as `name (1)`), `"replace"` (copied over it; the original goes
to the Trash once the copy holds the name) or `"skip"`; `otherwise` (default `"keepBoth"`) covers the rest. A copy
with `conflict: "merge"` is a merge keeping both everywhere. A merge goes on past an item that fails; its outcome is
then `failed`, with the first error, and what it did is still filed for undo.

Batches writing to the same drive run one after another, in the order they were started; a batch that has to
wait sends `job.queued {job, ahead}` before `job.started`. Batches for different drives run side by side.

`names: "fix"` saves a name the destination drive cannot store under the closest one it can; `"keep"` fails such
an item instead (meant for undo). Notifications: `job.started {job, total, totalBytes, totalFiles}`, `job.progress {job, item, bytes, files, flushes}`, `job.flushing {job}`,
`job.itemDone {job, index, outcome}`, `job.finished {job, done, failed, notAttempted, cancelled, undo, durable, settled?}`, where `undo` is the batch's
history entry, or `null` when nothing it did can be undone (with `undoError` if the history could not be
written), and for a durable batch `settled` says whether the drive confirmed everything was written
(`settleError` if not): only then is a removable drive safe to unplug. Every item gets
exactly one `job.itemDone`; its outcome is `{status: "done", path, renamed, skipped, kept, originalDeleted}`,
`{status: "failed", error, renamed, skipped, kept, originalDeleted}` or `{status: "notAttempted"}`, where `renamed` is
`[{requested, savedAs, cause}]`, `skipped` `[{path, kind}]`, and `kept` `[{path, cause: {reason, …}}]`: source
entries a move left in place (changed after the copy read them, undeletable, still open elsewhere, …), and an
original a replace could not delete on a drive with no Trash, left in an originals folder beside the target.
`leftOut` is true when the item itself was left out, being a link the destination cannot hold: it is in `skipped`
(or, for a move, `kept`), and nothing was made for it. `originalDeleted` is true when a replace deleted the original, its drive having no Trash: undo cannot bring it
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

Before anything is written, a batch measures what its items hold, without following links: `totalBytes` and
`totalFiles` (everything but folders) are `null` when an item could not be measured, and for a batch that only
trashes or deletes. An item holding a file larger than its destination drive can store (FAT32: 4 GiB) fails at
once with a message naming the file, unless it is already on that drive; the others go ahead. `job.progress`
counts each item on its own; `flushes` counts the waits it began while its drive wrote what it was given (before a
moved source is deleted, say), and `job.flushing` says a durable batch waits for its drive before it finishes.

Items may also take `name` (the name a conflict prompt offered; one plain name), and moves take `conflict`
`"fail"`, `"keepBoth"` or `"replace"`; a `merge` item with `move: true` merges by moving.

## Rename and create

Each answers from a thread of its own (a share may make it wait) and is filed for undo; `undo` is its history
entry's id, or `null` with `undoError` when the history could not be written. `label` names it in the history.

| Method | Params | Result |
|---|---|---|
| `ops.rename` | `path`, `name`, `label?` ("Rename") | `{path, undo}` |
| `ops.create` | `into`, `name`, `kind: "folder"\|"file"`, `unique?` (false), `label?` ("New Folder", "New File") | `{path, undo}` |

`ops.rename` renames within the item's folder, never over what holds the name (kind `exists`, "“b.txt” already
exists"). A name its drive would store otherwise (a character FAT cannot hold, too long for the share, a reserved
word) is refused before anything changes, kind `invalidFilename`, with the reason. On a drive that ignores case, a
change of case alone renames the item. `ops.create` makes an empty folder or file, never over anything: with
`unique`, as for a default name, a taken name becomes "New Folder (1)" and so on; without, as for a typed name, it
is refused like a rename's. Undo of a rename gives the old name back; undo of a create takes away the item while it
is unchanged (an edited file or a folder with something in it is left, in `left`), and redo makes it again.

## Undo

One history per session, shared by every Files process (`$XDG_RUNTIME_DIR/asc-files/undo-journal`), 50 entries deep.
Every batch is filed when it finishes, even a cancelled or partly failed one. A merge is filed item by item:
what it copied in and what it replaced; the folder merged into is never undone.

| Method | Params | Result |
|---|---|---|
| `undo.history` | – | `{undo: {id, label} \| null, redo: {id, label} \| null}` |
| `undo.undo` | – | `{id, label, done, left: [{path, error}], renamed, kept}` |
| `undo.redo` | – | the same |

Undo reverses the newest entry, newest step first: a copy goes to the Trash (or, on a drive without one, an
unchanged file is deleted), a move goes back under its old name, a trashed item is restored, and a replace puts
the original back and the copy in the Trash. A step whose item changed since is left as it is and listed in
`left`; the rest still run. A move gives back the names it saved entries under where the destination refused
them; one whose name is taken now, or which this drive refuses, keeps its saved name and is listed in `renamed`
(`cause` `taken`, `invalid` or `tooLong`), one that could not be renamed otherwise in `kept` (`nameNotRestored`),
and one gone since is left out. Redo does the entry again and never replaces what holds a name. Nothing to undo or
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
| `leftovers.review` | `folder`, `names` (leftovers listed there) | `{leftovers: [{name, modified, inside}]}`, `inside` `{kind: "item"}`, `{kind: "items", names, more}` (up to 1,000 names, `more` the count left out; a merge's by their path inside it), `{kind: "stillOpen", reason}`, `{kind: "gone"}` or `{kind: "unreadable", reason}` |
| `leftovers.recover` | `path` (an originals folder) | `{restored, renamed, kept, failed: [{path, error}], notAttempted, removed}` |
| `leftovers.sweep` | `folder`, `names` | `null` |

`leftovers.recover` puts each original back under its name beside the folder (a merge's into the folder it came
from), and never over an item that holds that name now: such an original comes back numbered, listed in
`renamed`. `removed` says the folder is gone, everything in it having been put back; `kept` lists what stays in
it and why (a file still open in a program, say). A path that is not an originals folder, or one holding only
another program's leftover, is refused. `leftovers.sweep` removes the probes among `names` that are empty and
more than an hour old, each tried once per backend.
