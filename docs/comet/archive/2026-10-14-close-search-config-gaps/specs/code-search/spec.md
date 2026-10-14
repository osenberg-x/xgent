# Code Search

## Purpose

This capability is the agent's in-repository text search: given a pattern and an optional starting directory, it walks the project and reports the matching lines so a model can locate code without reading whole files. It must be able to express the kinds of queries a programmer actually writes — alternation, anchors, wildcards in file names, "only Python files" — and it must never let a broken query masquerade as an empty result.

The behavior lives in `crates/xgent_tools/src/builtins/search_files.rs` and reaches the model through the `Tool` trait's `schema` and `execute`.

## Interface

The tool keeps its identity and safety posture unchanged: id `search_files`, `ToolTier::Read`, `Concurrency::Shared`, and the same confirmation policy as any other tool (default `NeedsConfirmation`).

Input arguments:

| Argument | Type | Default | Meaning |
| --- | --- | --- | --- |
| `pattern` | string | required | Text or regular expression to match |
| `path` | string | `"."` | Search root, resolved inside the project |
| `regex` | boolean | `false` | Treat `pattern` as a regular expression instead of a literal substring |
| `case_insensitive` | boolean | `false` | Ignore case in literal and regex matching |
| `glob` | string | absent | Keep only files whose project-relative path matches the glob |
| `file_type` | string | absent | Keep only files of a named type |

Output stays `path:line: text` lines, project-relative, forward-slash separated, followed by the same truncation notice when the result cap is reached.

The `ToolSchema` description states the defaults and warns that an invalid `regex` or `glob` is an error rather than a miss, because the model chooses these arguments.

## Literal matching stays the default

A pattern is text the model typed, and the overwhelmingly common call is `"pattern": "some_identifier"`. That call must keep meaning exactly what it meant before.

- With `regex` absent or `false`, matching is a substring test and is case-sensitive.
- Ignored directory names, the per-file size skip, the depth bound, the result cap, the cancellation boundaries and the truncation notice are unchanged.
- No existing caller of `search_files` needs to change.

Acceptance: A1

Scenario: plain pattern still matches literally
验收：A1

Given a project containing `src/a.rs` with the line `fn foo() {}`
When `search_files` runs with only `"pattern": "foo"`
Then the output contains `src/a.rs:` and that line, formatted as before

Scenario: a metacharacter in a literal pattern is literal
验收：A1

Given a file containing the text `a+b` and another containing `aab`
When `search_files` runs with `"pattern": "a+b"` and no `regex` argument
Then only the file with the literal `a+b` is reported

## Regular-expression matching

With `regex` set, the pattern is a Rust regular expression evaluated against each line independently.

- Matching is per line; a pattern is never applied across line boundaries, and the reported line is the line that matched.
- Regex features that `regex` supports and that do not span lines are available: anchors, character classes, alternation, repetition, non-capturing groups.
- Lookaround and backreferences are outside the `regex` engine's language; a pattern using them is reported as the same kind of invalid-pattern error as unbalanced parentheses.
- With `regex: true` and an invalid pattern, the result is `is_error`, its output names the `pattern` argument and the engine's reason, and the search returns before any directory is read.
- Case folding follows `case_insensitive`, not the regex mode.

Acceptance: A2

Scenario: anchored regex finds what substring search cannot
验收：A2

Given a project where several lines contain the substring `foo` but only one starts with `fn foo`
When `search_files` runs with `"pattern": "^fn\\s+\\w+"` and `"regex": true`
Then exactly that one line is reported

Scenario: an invalid regex is an error, not a miss
验收：A2

Given a pattern of `(foo` with `"regex": true`
When `search_files` runs
Then the result is an error mentioning the `pattern` argument
And the output does not claim 未找到匹配
And no file under the search root was opened

## Case-insensitive matching

`case_insensitive` is an independent switch that applies to both matching modes.

- In literal mode the comparison folds case without changing which text is reported.
- In regex mode it is the engine's own case-insensitive flag.
- Default `false` preserves today's case-sensitive behavior.
- Non-ASCII folding is handled by the engine in regex mode; in literal mode folding is Unicode-aware and matching offsets stay on character boundaries.

Acceptance: A3

Scenario: one flag, both modes
验收：A3

Given a file containing `FOO`, `foo` and `Foo` on separate lines
When `search_files` runs with `"pattern": "foo"` and `"case_insensitive": true`
Then all three lines are reported
And the same call without the flag reports only the exact `foo` line

Scenario: case folding in regex mode
验收：A3

Given a file containing `Search_Files`
When `search_files` runs with `"pattern": "search_files"`, `"regex": true` and `"case_insensitive": true`
Then that line is reported

## Glob filtering

`glob` restricts which files are searched by their path relative to the search root, using `/` as the separator on every platform.

- `*` matches any run of characters except `/`; `?` matches one character except `/`; `**` matches any run including `/`.
- `{a,b}` alternates between `a` and `b`.
- A pattern containing no `/` is matched against the file name alone, so `*.rs` finds `src/a.rs` and `deep/nested/b.rs`.
- A pattern containing `/` is matched against the whole project-relative path, so `src/**/*.rs` excludes `other/x.rs`.
- Glob matching is case-sensitive, independent of `case_insensitive`, which describes the content search.
- An invalid glob — unbalanced braces, a dangling `**` misuse, or a character class that cannot compile — is an error naming the `glob` argument, raised before traversal.
- A file skipped by the glob contributes nothing; ignored directories stay ignored whether or not the glob would have matched them.

Acceptance: A4

Scenario: extension glob narrows the search
验收：A4

Given a project with `src/a.rs` and `src/b.ts` both containing `foo`
When `search_files` runs with `"pattern": "foo"` and `"glob": "*.rs"`
Then only `src/a.rs` is reported

Scenario: path glob narrows the directory
验收：A4

Given a project with `src/deep/x.rs` and `tests/x.rs`, both containing `needle`
When `search_files` runs with `"pattern": "needle"` and `"glob": "src/**/*.rs"`
Then only the file under `src/` is reported

Scenario: an invalid glob is an error
验收：A4

Given `"glob": "src/{a,b"` with a valid `pattern`
When `search_files` runs
Then the result is an error mentioning the `glob` argument and no file is searched

## File-type filtering

`file_type` names a set of extensions, following ripgrep's `--type` vocabulary so a model can use what it already knows.

- Supported names and their extensions: `rust` (`rs`), `python` (`py`, `pyi`), `javascript` (`js`, `mjs`, `cjs`), `typescript` (`ts`, `mts`), `tsx` (`tsx`), `jsx` (`jsx`), `go` (`go`), `java` (`java`), `c` (`c`, `h`), `cpp` (`cpp`, `cc`, `cxx`, `hpp`, `hh`, `hxx`), `markdown` (`md`), `json` (`json`), `toml` (`toml`), `yaml` (`yaml`, `yml`), `shell` (`sh`, `bash`, `zsh`), `html` (`html`, `htm`), `css` (`css`, `scss`), `sql` (`sql`).
- Matching compares the file's extension case-insensitively.
- An unknown name is an error whose output lists the supported names; a valid name matching no file in the project is a normal empty result, not an error.
- When `file_type` and `glob` are both given, a file must satisfy both to be searched.

Acceptance: A5

Scenario: type filter selects by extension
验收：A5

Given a project with `main.py` and `main.rs` both containing `def_or_fn`
When `search_files` runs with `"pattern": "def_or_fn"` and `"file_type": "python"`
Then only `main.py` is reported

Scenario: an unknown type name is an error
验收：A5

Given `"file_type": "foobar"`
When `search_files` runs
Then the result is an error naming the `file_type` argument and listing the supported type names

## Cancellation and bounds under the new filters

The new arguments narrow what is searched; they do not change when the search stops.

- The cancellation signal is still honoured at directory and file boundaries, and a cancelled search still returns `ToolError::Aborted`.
- `MAX_RESULTS` still bounds reported matches and the truncation notice still appears only when the cap was reached; filtered-out files do not consume the cap.
- The depth bound and the per-file size skip still apply to the files that survive the filters.

Acceptance: A1

Scenario: abort during a filtered search
验收：A1

Given a search with `"regex": true` and a valid pattern whose cancellation token is cancelled during traversal
When `search_files` runs
Then it returns the aborted error rather than a partial result presented as complete

## Regression expectations

- The existing tests in `search_files.rs` keep passing; an assertion is adjusted only where it encoded the defect being closed.
- `xgent_context`'s keyword search is not touched by this capability: it still invokes `rg` with `--fixed-strings`, and its own traversal keeps its own ignore rules.
- `xgent_tools` keeps its dependency shape: the only new dependency is `regex`, declared in `[workspace.dependencies]`.
