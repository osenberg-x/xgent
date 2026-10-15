# File Checkpoint Undo

## Purpose

Everything the agent writes to the user's project is currently irreversible: `write_file` and `edit_file` overwrite files, the editor's undo stack only covers keystrokes inside a buffer, and the `<name>.xgent-bak` file the write path drops next to every target is read by nothing. This capability makes the agent's file changes reversible at the granularity of one conversation turn, driven entirely by the user.

It lives in three places: the snapshot and restore logic in `crates/xgent_tools/src/checkpoint.rs` (no Bevy, testable standalone), the turn bookkeeping in `crates/xgent_agent`, and one command palette entry in `crates/xgent_ui`.

## Storage layout

Per project, inside the directory the app already owns and the retrieval layer already ignores:

```
<project>/.xgent/checkpoints/
  turns/<session_id>/<turn_no>.json     # one manifest per turn
  blobs/<session_id>/<turn_no>-<idx>.bin # pre-write bytes, one per entry
```

A manifest is a small JSON object: session id, turn number, and an ordered list of entries. Each entry records the file path **relative to the project root**, whether the file existed before this write (`created` or `modified`), the blob holding its pre-write bytes (absent for `created`), the hash of the pre-write bytes, and the hash of the bytes the agent wrote.

Paths outside the project root cannot be recorded: every path goes through the same `resolve_in_project` the write tools use, so a checkpoint entry can never name a file the project does not own.

## Recording a change

Before a write tool is allowed to run, the executor records what is on disk right now.

- Recording happens after the policy decision (approved, or confirmed by the user) and immediately before `Tool::execute` — a denied or cancelled call records nothing.
- The snapshot is written and flushed before the target is touched. If the target write then fails, the entry still describes the true pre-state.
- All execution paths take the snapshot: the auto-approved path, the Allow path, and the AllowAll path. There is one place that does it, not three copies.
- Which files a call will write is declared by the tool itself through `Tool::write_targets(input, ctx)`, defaulting to "none"; `write_file` and `edit_file` declare theirs. A read-only tool declares nothing and is unaffected.
- Every write inside one turn is recorded as its own entry, in order, so a file written three times has three entries.

Acceptance: A1

Scenario: a successful write leaves a restorable record
验收：A1

Given a project whose file `src/a.rs` contains `one`
And the agent's `write_file` call for that path is approved
When the write completes with content `two`
Then the turn's manifest contains an entry for `src/a.rs` of kind `modified`
And the entry's blob holds the bytes `one` and the entry records the hash of `two`

Scenario: a declared target for every write tool
验收：A1

Given the built-in tools `write_file` and `edit_file`
When `write_targets` is asked with a valid `path` argument
Then it returns the resolved absolute path, and a path escaping the project root resolves to nothing

Scenario: a read-only tool declares no targets
验收：A1

Given the tools `read_file`, `search_files` and `run_command`
When `write_targets` is asked with any input
Then the result is empty, so no checkpoint entry is created for those calls

## What replaces the backup file

The single-slot side-by-side backup is retired rather than kept alongside.

- No `<name>.<ext>.xgent-bak` file is created by any write path any more, and `backup_existing` no longer exists for callers to reach.
- The crash-safety the backup nominally provided is provided better: the pre-state is durable before the target is written, and it survives more than one write per file.
- Files already left behind in a user's project are not deleted by this capability; that is the user's call.

Scenario: nothing strays into the project tree
验收：A1

Given a temporary project
When the agent writes a file twice in a row through `write_file`
Then no file matching `*.xgent-bak` exists anywhere under the project root
And both pre-states are recoverable from the checkpoint store

## Undoing a turn

Undo restores the newest recorded turn of the current session, one turn at a time.

- The unit is the turn manifest; undo replays its entries newest-first and then removes the manifest and its blobs.
- Repeated invocations walk backwards through the retained turns of that session. When nothing is recorded for the session, the action says so.
- Restoring a `modified` entry writes the recorded pre-state atomically.
- Removing a `created` entry deletes the file, but only when the file still holds what the agent wrote.
- Undo changes files on disk and nothing else: no git command, no editor buffer manipulation, no daemon round trip.

Acceptance: A2

Scenario: three writes in one turn return to the state before the turn
验收：A2

Given a file with content `c0`
When one turn writes `c1`, then `c2`, then `c3` to that same file
And the user undoes that turn
Then the file contains `c0`

Scenario: undo pops the turn it applied
验收：A6

Given a session with recorded turns 1 and 2
When the user undoes twice
Then turn 2's files are restored first, then turn 1's
And the third undo reports there is nothing left to undo for that session
And the manifests and blobs of the undone turns are gone

## Conflicts and refusals

Undo must never destroy work it did not create.

- Before restoring an entry, the current bytes of the file are hashed and compared with the hash the agent wrote. A mismatch means someone else changed the file, and the entry is skipped, not overwritten.
- A `modified` entry whose file no longer exists is skipped: the user deleted it deliberately, and recreating it would be a surprise.
- A `created` entry whose file no longer exists is already undone and is reported as such without touching disk.
- Skipping one file never prevents the others from being restored; the report lists restored and skipped paths separately.

Acceptance: A4

Scenario: an externally edited file is left alone
验收：A4

Given the agent wrote `agent-chosen` into `notes.md`
And afterwards the user edited that file to `user-chosen`
When the user undoes that turn
Then `notes.md` still contains `user-chosen`
And the undo report names `notes.md` as skipped

Acceptance: A3

Scenario: a file the agent created is removed by undo
验收：A3

Given `new.rs` did not exist before the turn and the agent created it
When the user undoes that turn
Then `new.rs` no longer exists

Scenario: a file the user deleted is not resurrected
验收：A3

Given `old.rs` existed before the turn, the agent overwrote it, and the user then deleted it
When the user undoes that turn
Then `old.rs` is still absent
And it is reported as skipped rather than silently rebuilt

## Degraded recording

The checkpoint mechanism must never become the reason a write fails.

- If the snapshot cannot be recorded (unwritable directory, disk full), the tool call still runs and its result is unchanged.
- The turn keeps a record that some of its writes were uncovered, and the undo report for that turn says so instead of claiming full coverage.

Acceptance: A7

Scenario: an unrecordable write is still performed and still reported honestly
验收：A7

Given a project whose checkpoint directory cannot be written
When the agent writes a file successfully
Then the file is written and the tool result reports success
And undo for that turn states that some files had no checkpoint rather than reporting nothing to undo

## Turn identity and lifetime

- A turn begins when the agent loop starts handling a user request for a session, and the store attributes every snapshot to it.
- The session id is the numeric `SessionId` passed through the start-loop command; it is never re-derived by parsing a formatted id, because the existing restore path parses `session:N` down to `0` and that mistake must not be copied into checkpoint keys.
- Undo only runs while the conversation is idle. While a turn is in flight it refuses with a message and changes nothing, so it cannot race the writer.
- A brand-new session (新建会话) resets the counter; each session's turns are undone independently, and switching sessions never exposes another session's checkpoints for undo.

Acceptance: A6

Scenario: a busy agent blocks undo
验收：A6

Given the conversation status is not Idle
When the user invokes undo
Then nothing on disk changes and the notice says the agent is still working

## Retention

- At most 20 turns per session are retained. Recording a 21st evicts the oldest manifest and its blobs.
- The bound follows from `MAX_WRITE_BYTES` (64KiB per file), so the worst case is roughly 1.3MiB per session; it is a named constant, not a configuration key in this round.

Acceptance: A8

Scenario: old turns fall out of the window
验收：A8

Given a session in which 25 turns each wrote one file
When retention is applied
Then manifests and blobs exist only for turns 6 through 25
And undo over the retained turns still restores the correct pre-state

## The user-facing command

- One command palette entry performs the undo, registered by id through `CommandRegistry::try_register` alongside the built-in commands and routed by the palette's existing trigger handler. It is not a `plugin.`-prefixed command.
- Its label is localised, never hardcoded.
- The result is reported through the existing toast: how many files were restored, and how many were skipped or never covered. Because the toast disappears in about two seconds, a run that skipped or missed anything also writes the detail into a conversation notice, in the same shape the loop-bound and compaction notices already use.

Acceptance: A5

Scenario: the command is discoverable and routes
验收：A5

Given the palette is open and the user types the undo command's label
When the entry is selected
Then a `PaletteTriggered` message carries the undo command id
And the handler performs a restore rather than routing to a plugin command

Acceptance: A10

Scenario: strings come from both catalogues
验收：A10

Given the palette label and every undo notice
When `zh-CN/main.ftl` and `en-US/main.ftl` are compared
Then both define the same new keys
And switching the interface language changes the shown text

## Relationship to the editor and the agent

- Undo writes through the same atomic path the write tools use: same-directory temp file, `sync_all`, rename, cleanup of the temp file on failure. An interrupted restore leaves the previous content intact and leaves no `*-xgent-tmp` behind.
- Undo does not add a second buffer-reload mechanism. The daemon's filesystem-watch pipeline already turns an external change into a reload for a clean open buffer and into the conflict dialog for a dirty one, and undo is just another external change. A dirty buffer is never silently replaced.
- The agent gains no way to undo. There is no tool the model can call, and the tool set and its schemas are otherwise unchanged.
- The editor's own `UndoStack` is untouched: it still covers in-buffer keystrokes and is still lost when a tab closes.

Acceptance: A9

Scenario: a reverted file already open stays consistent
验收：A9

Given `src/a.rs` is open in the editor and clean
When undo restores an earlier version of that file on disk
Then the existing watch pipeline reloads the buffer to the restored content
And if the buffer had unsaved edits, the existing conflict dialog appears instead of the edits being replaced

## Regression expectations

- `write_file` and `edit_file` keep their behaviour, their confirmation policy, their size limits and their diff preview; only the backup side effect disappears.
- The executor's confirmation semantics added this month stay: the timeout covers the whole wait for a decision, cancellation still wins, and a denied call records no checkpoint.
- `xgent_tools` keeps its dependency shape (no new dependency), and the checkpoint module stays Bevy-free.
- The workspace still compiles and the touched crates' tests still pass.

Acceptance: A11

Scenario: nothing else broke
验收：A11

Given the checkpoints and the undo command are implemented
When `cargo test -p xgent_tools --lib`, `cargo test -p xgent_agent --lib`, `cargo test -p xgent_ui --lib` and `cargo check --workspace --all-targets` are run
Then each reports no failures
