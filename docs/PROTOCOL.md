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
| `jobs.start` | `into`, `names?: "fix"\|"keep"` (fix), `items: [{source, action: "copy"\|"move"\|"replace"\|"trash", conflict?: "fail"\|"keepBoth"\|"merge"}]` | `{job}` |
| `jobs.cancel` | `job` | `null` |

`names: "fix"` saves a name the destination drive cannot store under the closest one it can; `"keep"` fails such
an item instead (meant for undo). Notifications: `job.started {job, total}`, `job.progress {job, item, bytes, files}`,
`job.itemDone {job, index, outcome}`, `job.finished {job, done, failed, notAttempted, cancelled}`. Every item gets
exactly one `job.itemDone`; its outcome is `{status: "done", path, renamed: [{requested, savedAs, cause}],
skipped: [{path, kind}]}`, `{status: "failed", error}` or `{status: "notAttempted"}`.

## Trash

| Method | Params | Result |
|---|---|---|
| `trash.list` | – | `{entries: [{info, file, original, deleted}]}` |
| `trash.restore` | `info` (an entry's `.trashinfo` path from `trash.list`) | `{path}` |
| `trash.delete` | `info` | `null` |
| `trash.empty` | – | `{deleted, failed}` |
