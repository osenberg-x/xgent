# Outcome

Make `xgent_context` retrieval work for the way users actually ask questions, and make its cost and thread behavior honest.

Four defects are closed in code, with tests:

1. A Chinese question produces keywords that can match nothing, because the whole sentence becomes one keyword (R3-1).
2. The `@file:` reference that the editor already sends arrives as a search keyword instead of as a file, so referencing a file does not put that file in context (R3-1 family).
3. Keyword patterns are handed to `ripgrep` as regex, so a keyword containing a metacharacter either matches the wrong thing or makes `rg` fail and trigger a full-tree scan (R3-1 family).
4. Directory traversal and the fallback search run `std::fs` calls directly inside `async fn`, blocking tokio worker threads (R3-8), and `ContextHub` sums provider budgets without rechecking the merged total (R3-9).
5. Every chunk reports the same hardcoded relevance text, so the LLM cannot tell why a file was included (R3-1, third bullet).

After this change lands, a keyword-free Chinese sentence still retrieves the file that contains its phrases, an `@file:` hint puts that file in context, `rg` is invoked literally, blocking IO leaves the async worker threads, the merged context respects its token budget, and each context chunk states which keywords it actually matched.

# Scope

## Sources and their role

`doc/notes/agent-gap-review.md` and `doc/notes/agent-gap-disposition.md` are triage input and an implementation reference, not a requirements source for this change. No full-coverage mapping table is maintained: the target is the five named defects above, all inside `crates/xgent_context`, and the disposition ledger already classifies every other entry in the report (R3-2 … R3-21 deferred, R4 roadmap). The gap report itself is not edited.

## In scope

| Gap | Change |
| --- | --- |
| R3-1 | CJK-aware keyword extraction: CJK runs become character bigrams, word length is measured in characters, stopword filtering applies to both scripts, and the keyword cap is raised to stay meaningful after bigram expansion |
| R3-1 | `relevance` reports the keywords a chunk actually matched, or marks the chunk as the current file, instead of one constant string |
| R3-1 (family) | `@file:<path>` hints are resolved to that file's content and are no longer used as search terms |
| R3-1 (family) | Keywords reach `ripgrep` as literal strings (`--fixed-strings` with one `-e` per keyword) so metacharacters neither mis-match nor fail the search |
| R3-8 | Directory traversal and fallback search run on the blocking pool via `tokio::task::spawn_blocking`; their filesystem calls live only in non-async functions |
| R3-9 | `ContextHub` rechecks the token budget over merged results: providers keep their order, chunks that do not fit are dropped, and the reported total is recomputed from the kept chunks |

## Out of scope but adjacent

- Real relevance ranking across providers (R3-2), which needs the retrieval-tier decision, and the `relevance` score numeric weighting beyond which keywords matched.
- `search_files` tool regex support (R3-3) — that is `xgent_tools`, a different crate and a different user path.

# Non-goals

- No new retrieval providers. `repo_map`, `vector`, `lsp`, `hybrid` stay the placeholders they are (R3-2, blocked on embedding/vector-store selection and D-06).
- No tokenization dependency. No `jieba-rs`, no ICU segmenter, no new workspace dependency; bigram segmentation is implemented in-crate.
- No configuration surface. The hardcoded retrieval parameters (`TREE_MAX_DEPTH`, `TREE_MAX_ENTRIES`, `RG_TIMEOUT_SECS`, `MAX_SEARCH_FILES`, budget) keep their current values and are not moved to config; that is R3-17.
- No change to `ContextQuery` / `ContextChunk` / `ContextResult` field types, to the `ContextProvider` trait, or to the WIT plugin contract.
- No change to how the agent injects context into the system message (`xgent_agent::format`), and no change to the agent's 8k retrieval budget.
- No parallel fan-out of providers inside `ContextHub`; providers stay sequential.
- No change to `doc/notes/agent-gap-review.md`.

# Constraints and invariants

- `xgent_context` keeps not depending on Bevy except where `ContextHub` already is a `Resource`; the retrieval logic itself stays testable without a window.
- UTF-8 character boundaries everywhere. Bigram slicing and truncation never split a character, and never panic on multi-byte input.
- The keyword cap is a bound on work, not a correctness rule: a query that produces more keywords than the cap still produces keywords in question order.
- Merged context never exceeds `query.max_tokens` in reported chunk tokens; dropping a chunk that does not fit is preferred over silently rewriting a provider's content.
- A provider that returns nothing keeps returning nothing; budget enforcement must not turn an empty result into an error.
- Blocking IO moves to the blocking pool without changing returned content: same tree text, same relative paths, same ignore rules, same `MAX_SEARCH_FILES`/visited-node bounds.
- No `unsafe`, no `unwrap()`/`expect()` outside tests, Chinese doc comments per `AGENTS.md` §5.8.
- New behavior is covered by tests in `xgent_context`, and every new test is checked to fail without the fix (the counter-proof method recorded in project memory).

# Decisions

- D-C1: CJK segmentation uses sliding character bigrams rather than a dictionary segmenter. A two-character slice is exactly what substring and `rg` literal matching can hit, it needs no lexicon or dependency, and it degrades to "no keyword" rather than "wrong keyword". Single-character CJK runs are dropped — a lone ideograph matches too much of a source tree to be a useful search term — so a one-character query yields no keywords and therefore no search chunks, while the directory tree and the current file are still returned.
- D-C2: Keyword count cap moves from 8 to 16 because bigram expansion multiplies the count for Chinese input; keeping 8 would truncate the second half of an ordinary Chinese question. The cap stays a single named constant.
- D-C3: Stopword filtering uses a short in-crate list of the most frequent Chinese function characters/words plus the common English function words. It is a precision filter, not a linguistic claim; unlisted function words remain harmless because they rarely match source content.
- D-C4: `@file:` handling stays provider-side. The hint prefix is recognized in `extract_keywords`/`retrieve` and resolved into the file's content; no new `ContextQuery` field is introduced, so plugin providers are unaffected.
- D-C5: `rg` is invoked with `--fixed-strings` and one `-e` per keyword instead of a `|`-joined regex. Multiple `-e` patterns are OR'ed by `rg`, which preserves current semantics while making each keyword literal, and it removes the malformed-pattern failure mode that currently falls back to a full-tree scan.
- D-C6: Budget recheck drops chunks instead of truncating them. Truncating another provider's content in the hub would corrupt the file excerpt the LLM is meant to read; dropping a whole chunk keeps every retained excerpt intact and is deterministic in provider order.
- D-C7: Merged `total_tokens` is recomputed as the sum of the kept chunks' `token_estimate`. Providers' own totals currently mix in the tree summary, so re-deriving from chunks is the only figure the hub can assert without re-estimating content.
- D-C8: The `spawn_blocking` split is structural: the sync traversal functions stay sync, are made private-pure over owned inputs, and are only called through `spawn_blocking`. Tests exercise the sync functions directly, so the behavior tests do not depend on thread scheduling.

# Acceptance examples

- A1: A Chinese user message with no spaces, whose phrases appear in a file's content, produces keywords that match that file, and the retrieved result includes that file. The same message before the change produced a single whole-sentence keyword and no match.
- A2: Keyword extraction measures word length in characters, drops CJK runs of one character, turns a CJK run of two or more characters into overlapping two-character slices, filters the listed Chinese and English stopwords, and returns at most 16 keywords in question order without duplicates.
- A3: A hint of the form `@file:src/main.rs` causes that file's content to appear as a context chunk, and the literal string `@file:src/main.rs` is never used as a search keyword.
- A4: Keywords containing regular-expression metacharacters (`+`, `<`, `>`, `(`, `.`, `/`) are searched as literals: the `rg` invocation uses fixed-string matching with one pattern per keyword, such a keyword neither matches unintended content nor causes a failed `rg` run that triggers the full-tree fallback.
- A5: Directory traversal and fallback search perform their filesystem calls in non-async functions that are invoked through `tokio::task::spawn_blocking`, and the tree text and search results they return are unchanged from before.
- A6: When merged chunks from several providers exceed `query.max_tokens`, the hub keeps chunks in provider order up to the budget, drops the chunks that do not fit without rewriting their content, and reports `total_tokens` as the sum of the kept chunks; when the merged result already fits, every chunk is kept unchanged.
- A7: Each returned chunk's `relevance` states what actually happened — the keywords the chunk content matched, or that it is the current file — and no chunk carries the previous constant string.
- A8: `cargo test -p xgent_context` passes with the new tests, and `cargo check --workspace` passes, so no consumer of `xgent_context` (`xgent_agent`, `xgent_app`, `xgent_plugin_host`) broke.
