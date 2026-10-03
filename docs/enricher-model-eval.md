# Enricher model evaluation

How to compare LLMs for the enricher (`crates/enricher`), which extracts a
category plus per-period dates, schedule windows, resolution status,
severity and impact type from Knowledgebase incident text through any
OpenAI-compatible endpoint (`LLM_BASE_URL` / `LLM_MODEL`).

Choosing a model asks two separate questions, and the harness keeps them
apart:

| | Quality eval | Performance benchmark |
| --- | --- | --- |
| Question | Does this model extract the right data? | Does this model, served from this environment, fit the service? |
| Depends on | The model (and prompt) | The model *and* the hardware/host/network/provider serving it |
| Timeout | Generous (`quality_timeout_secs`, default 1800 s) | The real `LLM_REQUEST_TIMEOUT_SECS` |
| Needs gold labels | Yes | No |
| Needs the model | Only to record outputs; scoring is offline-capable | Yes |
| Runner | `eval::quality::live_eval_quality`, `eval::quality::replay_quality_score` | `eval::perf::live_eval_perf` |
| Reports | `target/enricher-eval/quality/<stamp>/` | `target/enricher-eval/perf/<stamp>/` |

Run quality once per model (on any host that can serve it, however slowly),
and perf once per model *per environment* you might deploy to.

## How it works

Code: `crates/enricher/src/eval/`. It is test-only, like the existing
`replay_eval` and `llm::tests::live_eval_*` evals. Each runner is an
`#[ignore]`d test, so neither `cargo test` nor CI ever calls a model.

- `pipeline.rs` runs the service's real pipeline minus the database: the
  primary pass, the resolution-adversarial pass, the severity-adversarial
  pass, then `combine::combine_periods`. It uses the same order and the same
  stop-on-failure points as `process_incident`. The prompts, JSON schemas,
  parsers and provider policy are the service's own (`LlmClient::*_raw` and
  `llm::parse_*` in `llm.rs`), so the harness can't drift from production.
- Every run is saved as a **record**: one JSONL line per (case,
  repetition), holding each pass's raw output or error, latency, outcome
  label and in-call retry count.
- What the service would have written is derived from a record by
  replaying it through the same parsers. Quality scoring is a pure function
  of records, so saved records can be re-scored without the model.
- `quality.rs` scores records against gold labels. `perf.rs` computes
  latency, outcome and throughput statistics. The two share only the
  pipeline, the dataset loader and the target config.

## Quick start

```sh
cp crates/enricher/eval/targets.example.toml crates/enricher/eval/targets.toml
$EDITOR crates/enricher/eval/targets.toml     # git-ignored; one [[targets]] per model x environment

# Quality: records every target's outputs, scores them, writes reports
EVAL_TARGETS=crates/enricher/eval/targets.toml \
  cargo test -p enricher --bin enricher eval::quality::live_eval_quality -- --ignored --nocapture

# Quality, offline: re-score saved records (e.g. after fixing a gold label)
EVAL_RECORDS=target/enricher-eval/quality/<stamp>/<target>.records.jsonl \
  cargo test -p enricher --bin enricher eval::quality::replay_quality_score -- --ignored --nocapture

# Performance: run from a host with the same network path to the endpoint as the enricher
EVAL_TARGETS=crates/enricher/eval/targets.toml \
  cargo test -p enricher --release --bin enricher eval::perf::live_eval_perf -- --ignored --nocapture
```

The crate is bin-only, so the `--bin enricher` flag is required. Relative
paths in the `EVAL_*` variables resolve from the workspace root. Each run
prints its Markdown reports and writes them, with JSON copies and the raw
records, to a new timestamped directory.

Without `EVAL_TARGETS`, the runners evaluate a single target built from
the service's own env vars: `LLM_BASE_URL`, `LLM_MODEL`, `LLM_API_KEY`,
`LLM_REQUEST_TIMEOUT_SECS` and the `LLM_*` provider-policy knobs. Name it
with `EVAL_TARGET_NAME` and describe its environment with
`EVAL_ENVIRONMENT`. This is the quickest way to check an existing
deployment's `.env`.

## Targets

A target is one model served from one endpoint in one environment. The
same model on a laptop and on a GPU host is two targets. The format is in
`crates/enricher/eval/targets.example.toml`. The keys mirror the service's
env vars, and every optional key defaults to the service's own default
(read from `Config`'s definitions, not copied), so an unset key measures
the deployment as it would really run.

- `environment`: free text (hardware, host, provider, quantization,
  network path). Fill it in, because perf numbers mean nothing without it.
- `api_key_env`: the *name* of the env var holding the key. Keys never go
  in the file.
- `request_timeout_secs`: used by the perf benchmark only.
  `quality_timeout_secs` is used by the quality eval.
- `EVAL_TARGET=a,b`: runs only the named targets.

## Running the quality eval

| Variable | Default | Meaning |
| --- | --- | --- |
| `EVAL_DATASET` | `crates/enricher/eval/dataset.jsonl` | Dataset to use |
| `EVAL_CASES` | all | Comma-separated case ids |
| `EVAL_REPEATS` | 1 | Repetitions per case. Use 3 or more to measure consistency (the service sends `temperature: 0`, but many backends are still non-deterministic) |
| `EVAL_CONCURRENCY` | 1 | Documents in flight at once. Only for speed: quality doesn't depend on it |
| `EVAL_DATE_TOLERANCE_MINS` | 0 | Treat predicted and gold instants this close as equal |
| `EVAL_OUT_DIR` | `target/enricher-eval` | Report root |
| `EVAL_RECORDS` | (replay only) | Comma-separated `*.records.jsonl` files to re-score |

Only labelled cases are run. Per target, the run writes
`<target>.records.jsonl`, `<target>.json` and `<target>.md`, plus a
`comparison.md` with one row per target.

**Offline re-scoring** (`replay_quality_score`) groups the records by the
target and model named on each line, and scores them against the
*current* dataset. Use it when a gold label changes, when you add scoring
logic, or to score records produced elsewhere. Records for case ids no
longer in the dataset are counted and skipped.

## Running the performance benchmark

| Variable | Default | Meaning |
| --- | --- | --- |
| `EVAL_PERF_REPEATS` | 3 | Repetitions per case (more gives steadier percentiles) |
| `EVAL_PERF_CONCURRENCY` | 1 | Documents in flight at once |
| `EVAL_PERF_WARMUP` | 1 | Untimed runs of the first case first, so a cold model load doesn't skew the numbers |
| `EVAL_DATASET`, `EVAL_CASES`, `EVAL_OUT_DIR` | as above | Cases do not need labels here |

Choose the concurrency to match the deployment. The service's stream loop
processes one incident at a time (concurrency 1). The hourly sweep and
the reclaim loop can overlap with it, so up to 3 documents can be in
flight, capped across all callers by `max_in_flight` (`LLM_MAX_IN_FLIGHT`).
Use 1 for steady-state latency. Use 3 for a burst, such as the full
re-extraction after a prompt version bump.

Run the benchmark from a host with the same network path to the endpoint
as the enricher pod. Use `--release`: client-side overhead is negligible
either way, but there's no reason to measure a debug build. Every record
is saved, and the saved records can be quality-scored offline, but any
timeouts then count against the model.

## The dataset

`crates/enricher/eval/dataset.jsonl` holds one case per line:

```json
{"id": "flat-eta-signal-failure", "tags": ["single_period", "eta"], "notes": "why/how labelled",
 "reference_date": "2026-04-01T07:45:00Z", "summary": "...", "description": "...",
 "expected": {"category_any_of": ["signal_failure"], "periods": [
   {"scope_hint": "only shown in reports",
    "date_range": {"from_date": null, "to_date": "2026-04-01T17:00:00Z"},
    "schedule_window": null, "resolution_status": "ongoing",
    "apparent_severity": "moderate_disruption", "impact_type": null}]}}
```

| Field | Meaning |
| --- | --- |
| `id` | Unique and stable. Records refer to it, so renaming a case orphans its recordings |
| `reference_date` | What `process_incident` passes as `first_seen_at`. The model resolves year-less dates and bare times against it |
| `tags`, `notes` | Optional. Tags slice the report (a "By tag" table). Notes record why the case exists and how it was labelled |
| `expected` | Optional. Without it, the case is perf-only |
| `expected.category_any_of` | Accepted categories. `category` is free text in the schema, so list synonyms. They're compared lowercased, with punctuation runs folded to `_` |
| `expected.periods` | Gold periods in text order. Leave it out to leave segmentation unlabelled |
| Per period: `date_range`, or `from_date` / `to_date` on their own | Full range, or just one side when the other is a judgment call |
| `schedule_window` | `{days_of_week (ISO 1-7), start_time, end_time ("HH:MM")}` or `null`. Day order doesn't matter |
| `resolution_status`, `apparent_severity`, `impact_type` | A value, or a list of acceptable values. `impact_type` lists may include `null` |

**Absent is not null.** A field left out is not scored. A field given as
`null` is scored: the model must return null. Label only what the text
settles, and use an accepted-values list where reasonable readers
disagree, for example `["moderate_disruption", "severe_disruption"]`.

**Date conventions** (from `PRIMARY_PROMPT`). Gold dates are UTC instants:

1. A year-less date resolves to the occurrence closest to
   `reference_date`.
2. A stated end day is inclusive, so `to_date` is the *following* day's
   00:00.
3. A bare date is a Europe/London day boundary converted to UTC: 23:00Z the
   previous day in BST, 00:00Z in GMT. Watch the clock changes, as in the
   `bst-to-gmt-crossover-two-periods` case.
4. An ETA ("expected to resume from 18:00") is `to_date` with
   `from_date: null`.

**Adding a case:**

1. Start from real incident text where possible. `scripts/export-incident-history-for-replay.sql`
   exports `incident_history` as JSONL, and `incident_id`, `summary`,
   `description` and `first_seen_at` map directly onto `id`, `summary`,
   `description` and `reference_date`. Strip anything identifying.
2. Label it by hand against the rules above. Write `notes` explaining
   anything non-obvious.
3. Add tags. Reuse the existing ones (`single_period`, `multi_period`,
   `negation`, `eta`, `schedule_window`, `bst`, `gmt`,
   `over_segmentation_trap`, `observational`, ...) so the "By tag" table
   stays useful.
4. Run `cargo test -p enricher eval::dataset`. The
   `shipped_dataset_is_valid` test rejects unknown enum values, bad
   weekdays or times, duplicate ids and unknown keys (typos).
5. Re-score existing records offline to see how each model does on the
   new case, without re-running anything (the new case shows up only in
   new runs).

The 15 seed cases cover the design doc's golden-corpus list (ongoing,
resolved, residual, an ETA, a schedule window narrower than the date
range, and the negation pairs "has now ended" / "not expected to end soon"
and "until 18:00" / "began at 18:00"). They also cover the existing
live-eval fixtures with their specs' expected segmentation, a GMT-season
case and a BST-to-GMT crossover. They are a starting point, not a
benchmark: real model choices need more cases, ideally drawn from real
incident history.

## Reading the quality report

A **completed** case is one the service would have written. Failures are
split in two:

- **Transport failure**: the call failed (timeout, HTTP error, empty
  content). That's an environment problem, so look at the perf report. It
  is excluded from the valid-output rate.
- **Invalid output**: the model answered, but with something the service
  rejects (malformed or schema-violating JSON, an empty `periods` array,
  or adversarial verdicts that don't align with the primary periods).
  That's a quality problem.

**Period matching.** Predicted periods are paired with gold periods
greedily, by how many scored fields agree (ties go to the closer
position). An unmatched prediction is a hallucinated (extra) period, and
an unmatched gold period is a missed one. Period precision is matched /
predicted, and period recall is matched / gold.

**Field verdicts**, per matched pair and scored field:

| Verdict | Meaning | Counts against |
| --- | --- | --- |
| correct | non-null and accepted | |
| correct_null | null, and null is accepted | |
| wrong | non-null, wrong value | precision and recall |
| hallucinated | non-null where gold is null | precision |
| missed | null where gold is non-null | recall |

Accuracy is (correct + correct_null) / scored. Precision and recall treat
a non-null value as a positive. They matter most for `date_range`,
`schedule_window` and `impact_type`, where returning null is a real
answer. Fields scored per period: `date_range` (whole range), `from_date`,
`to_date`, `schedule_window`, `impact_type`, `resolution_status`,
`apparent_severity`. `scope_description` is free text and never scored.

Other sections:

- **Exact case match**: completed, every scored field correct, and the
  right period count. Observational cases (nothing labelled) are excluded.
- **Wrong dates by kind** shows how the dates went wrong: `off_by_1h`
  (Europe/London to UTC conversion ignored), `off_by_1d` (end day not
  treated as inclusive), `wrong_year` (year inference) or `other`.
- **Confusions** lists gold value to predicted value for the enum fields.
  A false `resolved` is the costliest mistake, because the aggregator can
  demote a status on it.
- **Low-confidence rates** show how often the adversarial passes disagreed
  with the primary pass. That isn't an error, but low-confidence fields
  are ignored downstream, so a very high rate means the extraction rarely
  takes effect.
- **Consistency** (with repetitions) shows whether repeats of a case gave
  the same output, using `churn::compare`. `scope_description` wording is
  ignored.
- **Cases** lists every mismatch for every attempt, with `scope_hint`
  labels.

## Reading the performance report

- **Calls**: per pass and overall, giving the outcome counts (the
  service's `enricher_llm_call_total` labels: `success`, `timeout`,
  `gateway_error`, `rate_limited`, `http_error`, `empty_content`,
  `error`), timeout and error rates, in-call retries, and latency
  percentiles. Percentiles use the nearest-rank method, over successful
  calls, with retries and 429 waits included, like
  `enricher_llm_call_duration_seconds`. "Max incl. failures" is how long
  a failing call held the loop.
- **Documents**: all three passes, end to end. Shows completion, failures
  by stage, latency and documents per minute at the chosen concurrency.
- **Timeout fit**: `exceeds` if any call hit the client timeout. `tight`
  if successful-call p95 is at least 80% of `request_timeout_secs`.
  Otherwise `fits`. The report also gives completed-document p95 as a
  share of `RECLAIM_MIN_IDLE_SECS`: a document slower than that gets
  reclaimed and skipped while it is still in flight. That's churn, not an
  error, but worth avoiding.
- A gateway that cuts requests itself (returning 504) shows up as
  `gateway_error`, not `timeout`. As `config.rs` advises, set the timeout
  just above the gateway's cutoff, and consider `gateway_retries`.

## Choosing a model

1. Shortlist models from the quality comparison: valid-output rate,
   `resolution_status` with no false `resolved`, segmentation (period
   precision/recall and the `multi_period` tag), and consistency across
   repeats.
2. For each shortlisted model, run perf in each candidate environment, at
   concurrency 1 and 3, with the timeouts you intend to deploy.
3. Deploy a model and environment pair that is both good enough and
   `fits`. Set `LLM_REQUEST_TIMEOUT_SECS` with headroom over the measured
   p95, and `RECLAIM_MIN_IDLE_SECS` above the document p95.
4. Keep the records. When the prompt, the dataset or the scoring changes,
   re-score offline first, and re-run models only when the prompt changes.

Changing `PRIMARY_PROMPT` or a schema changes every model's behaviour, so
re-run the quality eval for each candidate afterwards.

## Tests

The scoring, matching, statistics, dataset validation and pipeline logic
are unit-tested with a scripted fake backend (`pipeline::FakeBackend`).
They run in the normal `cargo test -p enricher`, with no network. The
real-model runners are `#[ignore]`d and named `live_eval_*` / `replay_*`,
the prefixes CI's ignored-tests step already skips
(`--skip live_eval --skip replay_`).

## Limitations and open questions

- **Prompt vs rule on BST.** `PRIMARY_PROMPT`'s worked examples write
  bare dates as `00:00Z`, but its rule (3) says to convert Europe/London to
  UTC (23:00Z in BST). The gold labels follow the rule, so a model that
  copies the examples scores `off_by_1h`. The *Wrong dates by kind* table
  makes this visible, and `EVAL_DATE_TOLERANCE_MINS=60` forgives it. The
  prompt should be made consistent either way.
- **Category is free text** (the schema has no enum), so category accuracy
  is only as good as each case's synonym list.
- **Greedy period matching** can mis-pair two near-identical periods. It
  is deterministic, and the per-case mismatch list shows the pairing.
- **The seed labels are judgments.** Contested fields use accepted-value
  lists or are left unscored. Review them as the dataset grows.
- **Small seed set.** 15 cases can separate a broken model from a good one,
  but not two good ones. Grow the set from real incident history first.
- **Throughput is client-side.** It reflects the endpoint as seen from the
  host running the benchmark, including any other load on a shared
  endpoint. Note that load in the target's `environment`.
