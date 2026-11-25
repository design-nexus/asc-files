#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes docs/protocol.schema.json, the JSON Schema of the backend protocol (PROTOCOL.md):
    scripts/protocol-schema.py > docs/protocol.schema.json
The backend tests check every message against it."""
import json, sys
def ref(n): return {"$ref": f"#/$defs/{n}"}
def obj(required=None, optional=None, extra=False):
    required = required or {}; optional = optional or {}
    o = {"type": "object", "properties": {**required, **optional}, "required": sorted(required)}
    if not extra: o["additionalProperties"] = False
    return o
INT = {"type": "integer", "minimum": 0}
SINT = {"type": "integer"}
BOOL = {"type": "boolean"}
STR = {"type": "string"}
NULL = {"type": "null"}
def nullable(s): return {"anyOf": [s, NULL]}
def arr(s): return {"type": "array", "items": s}
def const(v): return {"const": v}
def enum(*v): return {"enum": list(v)}
P, N, E = ref("path"), ref("name"), ref("error")
KINDS = ["failed","notFound","exists","isDirectory","notDirectory","notEmpty","notRegularFile","notSymbolicLink","filenameTooLong","invalidFilename","tooManyLinks","noSpace","invalidArgument","permissionDenied","notSupported","notMounted","cancelled","readOnly","timedOut","busy","wouldBlock","tooManyOpenFiles","invalidData","brokenPipe","noSuchDevice","symlinkUnheld","passwordRequired","passwordIncorrect"]
special = enum("pipe","socket","device","symlink")
renamed = obj({"requested": P, "savedAs": P, "cause": enum("alias","taken","invalid","tooLong")})
kept_cause = {"oneOf": [
    obj({"reason": enum("changed","replacementChanged","copyGone")}),
    obj({"reason": const("undeletable"), "error": STR}),
    obj({"reason": const("skipped"), "kind": special}),
    obj({"reason": const("stillOpen"), "program": nullable(STR)}),
    obj({"reason": const("nameNotRestored"), "savedAs": nullable(P), "error": STR}),
    obj({"reason": const("originalNotRemoved"), "error": STR}),
]}
kept = obj({"path": P, "cause": ref("keptCause")})
changes = {"renamed": arr(ref("renamed")), "skipped": arr(obj({"path": P, "kind": special})), "kept": arr(ref("kept")), "originalDeleted": BOOL, "leftOut": BOOL}
outcome = {"oneOf": [
    obj({"status": const("done"), "path": P, **changes}),
    obj({"status": const("failed"), "error": E, **changes}),
    obj({"status": const("notAttempted")}),
]}
sort = obj({}, {"key": enum("name","size","modified","type"), "descending": BOOL, "foldersFirst": BOOL})
row = obj({"name": N, "kind": enum("folder","file","link","special"), "linkToFolder": BOOL, "hidden": BOOL},
          {"size": INT, "modified": SINT, "modifiedNs": INT, "mode": INT, "gone": const(True)})
clash_choice = enum("keepBoth","replace","skip")
item = obj({"action": enum("copy","merge","move","replace","trash","delete","restore","extract","compress")},
           {"source": P, "conflict": enum("fail","keepBoth","merge","replace"), "name": N, "move": BOOL,
            "password": STR, "sources": arr(P), "archiveName": STR, "format": enum("zip","7z","tar","tar.gz"),
            "choices": {"type": "object", "additionalProperties": clash_choice}, "otherwise": clash_choice, "to": P})
side = obj({"isFolder": BOOL, "size": INT, "modified": SINT})
history_entry = nullable(obj({"id": INT, "label": STR}))
undone = obj({"id": INT, "label": STR, "done": INT, "left": arr(obj({"path": P, "error": E})), "renamed": arr(ref("renamed")), "kept": arr(ref("kept"))})
filed = obj({"path": P, "undo": nullable(INT)}, {"undoError": E})
names = arr(N)
inside = {"oneOf": [
    obj({"kind": enum("item","gone")}),
    obj({"kind": const("items"), "names": arr(N), "more": INT}),
    obj({"kind": enum("stillOpen","unreadable"), "reason": STR}),
]}
none = obj()
methods = {
 "initialize": (obj({}, {"protocolVersion": INT}), obj({"protocolVersion": const(2), "server": STR, "capabilities": arr(STR)})),
 "shutdown": (none, NULL),
 "list.open": (obj({"path": P}, {"showHidden": BOOL, "sort": sort, "filter": STR}), obj({"listing": INT, "generation": INT, "count": INT, "followsChanges": BOOL, "leftovers": arr(N)})),
 "list.window": (obj({"listing": INT, "start": INT, "count": INT}), obj({"listing": INT, "generation": INT, "start": INT, "rows": arr(ref("row"))})),
 "list.reorder": (obj({"listing": INT}, {"showHidden": BOOL, "sort": sort, "filter": STR}), obj({"generation": INT, "count": INT, "leftovers": arr(N)})),
 "list.refresh": (obj({"listing": INT}), obj({"generation": INT, "count": INT, "leftovers": arr(N)})),
 "list.close": (obj({"listing": INT}), NULL),
 "jobs.start": (obj({"into": P, "items": arr(ref("item"))}, {"names": enum("fix","keep"), "label": STR, "durable": BOOL}), obj({"job": INT})),
 "jobs.cancel": (obj({"job": INT}), NULL),
 "jobs.clashes": (obj({"into": P, "sources": arr(P)}), obj({"items": arr({"oneOf": [
     obj({"source": P, "clashes": arr(obj({"path": P, "incoming": side, "existing": side}))}),
     obj({"source": P, "error": E})]})})),
 "jobs.plan": (obj({"into": P, "sources": arr(P)}), obj({"items": arr({"oneOf": [
     obj({"source": P, "name": N, "renamed": BOOL, "incoming": side, "existing": nullable(side), "same": BOOL, "aliasOf": nullable(INT)}),
     obj({"source": P, "error": E})]})})),
 "search.start": (obj({"roots": arr(P), "query": STR}, {"hidden": BOOL, "limit": INT, "entries": INT, "seconds": INT, "exclude": arr(STR)}), obj({"search": INT})),
 "search.exclusion": (obj({"rule": STR}), obj({"rule": STR})),
 "measure.start": (obj({"path": P}), obj({"measure": INT})),
 "measure.cancel": (obj({"measure": INT}), NULL),
 "ops.chmod": (obj({"path": P, "bits": INT}), obj({"mode": INT})),
 "actions.list": (obj({}, {"inputs": arr(obj({"path": P}, {"folder": BOOL, "type": STR}))}), obj({"actions": arr(ref("action")), "failures": arr(obj({"directory": STR, "reason": STR}))}, {"matched": arr(STR)})),
 "actions.read": (obj({"id": STR}), obj({"definition": {"type": "object"}, "script": nullable(obj({"fileName": STR, "contents": STR}))})),
 "actions.write": (obj({"definition": {"type": "object"}}, {"script": nullable(obj({"fileName": STR, "contents": STR})), "create": BOOL}), obj({"id": STR})),
 "actions.setEnabled": (obj({"id": STR, "enabled": BOOL}), NULL),
 "actions.delete": (obj({"id": STR}), NULL),
 "actions.duplicate": (obj({"id": STR}), obj({"id": STR})),
 "actions.import": (obj({"path": P}), obj({"id": STR})),
 "actions.export": (obj({"id": STR, "into": P}), obj({"path": P})),
 "actions.examples": (obj(), arr(obj({"name": STR, "category": STR, "description": STR, "requirements": STR, "inputs": STR, "extensions": arr(STR), "folders": BOOL, "mode": STR, "runtime": STR, "script": STR}))),
 "actions.run": (obj({"id": STR, "inputs": arr(obj({"path": P}, {"folder": BOOL, "type": STR})), "parent": P}, {"background": BOOL}), obj({"job": INT})),
 "actions.cancel": (obj({"job": INT}), NULL),
 "actions.dismiss": (obj({"job": INT}), NULL),
 "search.cancel": (obj({"search": INT}), NULL),
 "ops.rename": (obj({"path": P, "name": STR}, {"label": STR}), filed),
 "ops.create": (obj({"into": P, "name": STR, "kind": enum("folder","file")}, {"unique": BOOL, "label": STR}), filed),
 "undo.history": (none, obj({"undo": history_entry, "redo": history_entry})),
 "undo.preview": (none, obj({"blocked": arr(P)})),
 "undo.undo": (obj({}, {"overwrite": BOOL}), undone),
 "undo.redo": (none, undone),
 "trash.list": (none, obj({"entries": arr(obj({"info": P, "file": P, "original": P, "deleted": STR}))})),
 "trash.restore": (obj({"info": P}), obj({"path": P})),
 "trash.delete": (obj({"info": P}), NULL),
 "trash.empty": (none, obj({"deleted": INT, "failed": arr(obj({"info": P, "error": E}))})),
 "leftovers.recover": (obj({"path": P}), obj({"restored": arr(P), "renamed": arr(ref("renamed")), "kept": arr(ref("kept")),
     "failed": arr(obj({"path": P, "error": STR})), "notAttempted": arr(P), "removed": BOOL})),
 "leftovers.review": (obj({"folder": P, "names": names}), obj({"leftovers": arr(obj({"name": N, "modified": nullable(SINT), "inside": inside}))})),
 "leftovers.sweep": (obj({"folder": P, "names": names}), NULL),
}
gone = obj({"listing": INT, "path": P, "nearest": P})
notifications = {
 "list.changed": obj({"listing": INT, "generation": INT, "count": INT, "leftovers": arr(N)}),
 "list.gone": gone,
 "list.unmounted": gone,
 "search.found": obj({"search": INT, "items": arr(ref("found"))}),
 "search.finished": obj({"search": INT, "items": arr(ref("found")), "matched": INT, "entries": INT, "truncated": BOOL, "cancelled": BOOL}),
 "action.job": obj({"job": INT, "name": STR, "icon": nullable(STR), "mode": STR, "parent": P, "status": enum("queued","running","cancelling","succeeded","failed","cancelled"), "completed": INT, "succeeded": INT, "failed": INT, "total": INT, "fraction": nullable({"type": "number"}), "step": nullable(STR), "message": nullable(STR), "log": STR, "logTruncated": BOOL, "created": arr(P), "elapsed": INT}),
 "measure.progress": obj({"measure": INT, "items": INT, "size": INT, "files": INT, "folders": INT}),
 "measure.finished": {"oneOf": [
     obj({"measure": INT, "items": INT, "size": INT, "files": INT, "folders": INT, "unreadable": BOOL, "timedOut": BOOL, "depthLimited": BOOL, "cancelled": BOOL}),
     obj({"measure": INT, "error": E})]},
 "job.queued": obj({"job": INT, "ahead": INT}),
 "job.started": obj({"job": INT, "total": INT, "totalBytes": nullable(INT), "totalFiles": nullable(INT)}),
 "job.progress": obj({"job": INT, "item": INT, "bytes": INT, "files": INT, "flushes": INT}),
 "job.flushing": obj({"job": INT}),
 "job.itemDone": obj({"job": INT, "index": INT, "outcome": ref("outcome")}),
 "job.finished": obj({"job": INT, "done": INT, "failed": INT, "notAttempted": INT, "cancelled": BOOL, "undo": nullable(INT), "durable": BOOL},
                     {"undoError": E, "settled": BOOL, "settleError": E}),
}
text = {"type": "string", "minLength": 1}
bytes_form = obj({"bytes": {"type": "string", "contentEncoding": "base64"}}, {"display": STR})
schema = {
 "$schema": "https://json-schema.org/draft/2020-12/schema",
 "$id": "https://github.com/design-nexus/asc-files/blob/main/docs/protocol.schema.json",
 "title": "Files backend protocol, version 2",
 "description": "Every message asc-files-backend sends and accepts. Requests are {jsonrpc, id, method, params}; a method's params and result are under $defs/methods, a notification's params under $defs/notifications. See PROTOCOL.md.",
 "$defs": {
  "path": {"description": "Absolute in requests. A string when UTF-8, else its bytes in base64 (with display text when sent by the backend).", "oneOf": [text, bytes_form]},
  "name": {"description": "One name within a folder: no '/', not '.' or '..'.", "oneOf": [text, bytes_form]},
  "errorKind": enum(*KINDS),
  "error": obj({"kind": ref("errorKind"), "message": STR}),
  "action": obj({"id": STR, "name": STR, "description": nullable(STR), "icon": nullable(STR), "enabled": BOOL, "menu": enum("submenu","top"), "runtime": STR, "mode": STR, "confirm": BOOL, "available": BOOL, "reason": nullable(STR)}),
  "found": obj({"path": P, "isFolder": BOOL, "size": INT, "modified": SINT, "score": SINT}),
  "renamed": renamed, "keptCause": kept_cause, "kept": kept, "outcome": outcome, "row": row, "item": item,
  "id": {"type": ["string", "integer", "null"]},
  "request": obj({"jsonrpc": const("2.0"), "method": STR}, {"id": ref("id"), "params": {"type": ["object", "null"]}}),
  "reply": {"oneOf": [
      obj({"jsonrpc": const("2.0"), "id": ref("id"), "result": True}),
      obj({"jsonrpc": const("2.0"), "id": ref("id"), "error": obj({"code": SINT, "message": STR}, {"data": True})})]},
  "operationError": obj({"code": const(1), "message": STR, "data": obj({"kind": ref("errorKind")})}),
  "notification": obj({"jsonrpc": const("2.0"), "method": enum(*notifications), "params": {"type": "object"}}),
  "methods": {m: {"params": p, "result": r} for m, (p, r) in methods.items()},
  "notifications": notifications,
 },
 "anyOf": [ref("request"), ref("reply"), ref("notification")],
}
json.dump(schema, sys.stdout, indent=2, ensure_ascii=False); print()
