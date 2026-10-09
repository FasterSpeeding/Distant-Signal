# Using the Claude API for the enricher

How to point the enricher (`crates/enricher`) at Anthropic's Claude API,
through either the normal (synchronous) route or the Message Batches route,
what prompt caching does for this workload, what it costs, and how to switch
back.

**Status (2026-10-09): not deployed, off by default.** `LLM_PROVIDER`
defaults to `openai` (any OpenAI-compatible endpoint, including the current
self-hosted model), and an unset deployment sends exactly the requests it
sent before. Nothing here has run against the real Claude API: there was no
key. Every request and response shape below is unit-tested against mocked
HTTP fixtures taken from the API reference. Run the
[quality eval](#evaluating-before-switching) before switching production.

Claude API documentation this page relies on (read 2026-10-09; re-check
before relying on prices):

- Models overview: <https://platform.claude.com/docs/en/about-claude/models/overview>
- Claude Haiku 5.5: <https://platform.claude.com/docs/en/models/haiku-5-5/overview>
- Claude Sonnet 5.5: <https://platform.claude.com/docs/en/models/sonnet-5-5/overview>
- Structured outputs: <https://platform.claude.com/docs/en/build-with-claude/structured-outputs>
- Prompt caching: <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>
- Message Batches: <https://platform.claude.com/docs/en/build-with-claude/batch-processing>
- Errors: <https://platform.claude.com/docs/en/api/errors>
- Data retention: <https://platform.claude.com/docs/en/manage-claude/api-and-data-retention>

## How it fits in

`llm.rs` has one client and two wire providers. Everything above the wire is
shared: the three prompts and schemas, the three calls per incident
(primary, resolution-adversarial, severity-adversarial), the provider policy
(`LLM_MAX_IN_FLIGHT`, `LLM_RATE_LIMIT_*`, `LLM_GATEWAY_RETRIES`), the typed
errors and their outcome labels, the per-text backoff, and the metrics.

| | `openai` (default) | `anthropic` |
| --- | --- | --- |
| Endpoint | `POST {LLM_BASE_URL}/chat/completions` | `POST {LLM_BASE_URL}/messages` (default base `https://api.anthropic.com/v1`) |
| Credential | `Authorization: Bearer` (or keyless WIF) | `x-api-key` + `anthropic-version: 2023-06-01` |
| System prompt | a `system` message | one `system` text block with `cache_control` (`LLM_PROMPT_CACHE`) |
| Structured output | `response_format: json_schema, strict: true` | `output_config.format: json_schema` (schema rewritten, below) |
| Temperature | `0` | not sent: the current Claude models reject a non-default one |
| `LLM_REASONING_EFFORT` | `reasoning_effort` | `output_config.effort` |
| `LLM_MAX_TOKENS` | `max_tokens`, omitted when unset | `max_tokens`, 16000 when unset (required by the API) |
| Thinking | n/a | `LLM_THINKING` → `thinking.type`; unset = model default (adaptive) |
| Bulk path | none | Message Batches for the sweep (`LLM_SWEEP_MODE=batch`) |

The code: `crates/enricher/src/llm/anthropic.rs` (request, response,
errors, batch client), `crates/enricher/src/batch.rs` (batch mode),
`crates/enricher/src/config.rs` (`resolve_llm`, the settings).

**Schemas.** The three schemas are written in OpenAI's strict subset. Claude
rejects two things in them with a 400: type unions (`"type": ["string",
"null"]`) and numeric bounds (`minimum`/`maximum` on `days_of_week`). For
Claude they are rewritten on the way out (`anthropic::claude_schema`): a
nullable scalar becomes `anyOf: [{"type": "string", ...}, {"type": "null"}]`
(an enum's `null` moves to the null branch), and a bound becomes description
text. The JSON the model returns has the same shape, so the parsers are
unchanged. `every_schema_is_rewritten_into_claudes_subset` checks all three.

**No temperature.** Sonnet 5.5 returns a 400 for a non-default
`temperature`, and Haiku 5.5's and Opus 5.5's docs say to omit it. So repeat
runs over the same text are not guaranteed identical, unlike the `openai`
path's `temperature: 0`. The per-text backoff and the combine-mismatch
tracker still work, but a "deterministic" persistent mismatch may now
sometimes clear on retry.

## Setup

1. Create a Claude API key in a dedicated workspace with a monthly spend
   limit (a few dollars a month covers the expected load, see [Cost](#cost)).
   A spend limit surfaces as HTTP 400, a billing problem as 402
   `billing_error` (`quota_exhausted`).
2. Store it in a Secret:
   `kubectl create secret generic enricher-anthropic --from-literal=anthropic-api-key=...`
3. Chart values:

   ```yaml
   enricher:
     llm:
       provider: anthropic
       anthropic:
         existingSecret: enricher-anthropic
         # model: claude-haiku-5-5        # the default
         # promptCache: 1h                # the default
       # reasoningEffort: ""              # optional, sent as output_config.effort
     llmRequestTimeoutSecs: 120
     extraEnv:
       - { name: LLM_MAX_IN_FLIGHT, value: "3" }
       - { name: LLM_RATE_LIMIT_RETRIES, value: "3" }
       - { name: LLM_GATEWAY_RETRIES, value: "2" }
   ```

   `enricher.llm.baseUrl`/`model`/`apiKey`/`existingSecret` are not used
   with `provider: anthropic`, and `enricher.llm.auth` must stay `apiKey`.
   If the opt-in egress NetworkPolicies are on, the enricher's internet rule
   already opens `enricher.llm.anthropic.baseUrl`'s port (443); batches use
   the same host.

Service env vars (the chart sets them; listed for local runs):

| Env var | Default | Notes |
| --- | --- | --- |
| `LLM_PROVIDER` | `openai` | `anthropic` for the Claude API |
| `LLM_BASE_URL` | `https://api.anthropic.com/v1` (anthropic) | required for `openai` |
| `LLM_MODEL` | `claude-haiku-5-5` (anthropic) | required for `openai`; also part of `model_version` |
| `LLM_API_KEY` | none | required for `anthropic` (sent as `x-api-key`) |
| `LLM_ANTHROPIC_VERSION` | `2023-06-01` | the `anthropic-version` header |
| `LLM_PROMPT_CACHE` | `1h` | `1h`, `5m` or `off`; see [Prompt caching](#prompt-caching) |
| `LLM_THINKING` | unset | `thinking.type`, e.g. `disabled` (Haiku 5.5), `between_tools` (Sonnet 5.5) |
| `LLM_SWEEP_MODE` | `sync` | `batch`: see [Batch mode](#batch-mode) |
| `LLM_BATCH_MIN_ITEMS` | `20` | a smaller sweep stays synchronous |
| `LLM_BATCH_MAX_ITEMS` | `2000` | incidents per batch, capped at 5000 |
| `LLM_BATCH_POLL_SECS` | `60` | poll interval for in-flight batches |

Startup refuses `anthropic` without `LLM_API_KEY`, with a workload identity
`LLM_AUTH` mode, and `LLM_SWEEP_MODE=batch` with `openai`; the chart refuses
the same combinations at render time.

### Errors and what the enricher does with them

Same outcome labels and retry budgets as the `openai` path
(docs/enricher-openai.md, "Errors"):

| Response | Outcome label | Retried in-call? | Per-text backoff? |
| --- | --- | --- | --- |
| 429 `rate_limit_error` | `rate_limited` | yes, `LLM_RATE_LIMIT_RETRIES`, honouring `retry-after` (≤ 600 s) | no |
| 402 `billing_error` | `quota_exhausted` | no | no |
| 529 `overloaded_error`, 500 `api_error`, 504 `timeout_error`, 502/503 | `gateway_error` | yes, `LLM_GATEWAY_RETRIES`, `retry-after` when sent, else 2 s, 4 s, ... | no (504 only with gateway retries on) |
| other 4xx (400 invalid request or spend limit, 401, 403, 404, 413) | `http_error` | no | yes |
| 200, `stop_reason: "refusal"` | `refused` (`stop_details.category: explanation` in the log) | no | yes |
| 200, `stop_reason: "max_tokens"`, or no text block | `empty_content` (`finish_reason` = the stop reason) | no | yes |

Failures are logged with the response's `request-id`, which Anthropic
support asks for. Responses also carry `thinking` blocks (adaptive thinking;
their text is empty by default): they are skipped, and only `text` blocks are
parsed.

## Batch mode

Message Batches run asynchronously at **50% of the synchronous price**, most
within an hour and all within 24 hours (results stay downloadable for 29
days).

**Where it applies: the sweep only.** The enricher has three loops: the
stream loop (a text change, seconds after `api` publishes it), reclaim (its
retries) and the hourly sweep (anything stale: missed events and, above all,
every live incident after a `model_version` change). Only the sweep is bulk
and latency-tolerant, so only the sweep batches; the other two always stay
synchronous. A sweep that finds fewer than `LLM_BATCH_MIN_ITEMS` (20)
incidents runs them synchronously too: in steady state the sweep finds a
handful, and half the price of a handful of calls isn't worth an hour's
delay. So in practice batch mode pays off on the **re-extraction after a
model or prompt change** (including the switch to Claude itself), which is
also when the volume is largest.

**How it runs.** An incident is three calls, and the two adversarial calls
need the primary call's periods, so a batch run has two stages:

1. The sweep drops every incident already in a batch, runs the usual
   preflight (unchanged text is skipped, semantic no-ops carried forward,
   backed-off texts skipped), and submits one primary request per incident
   (`custom_id` `p-<n>`), up to `LLM_BATCH_MAX_ITEMS` per batch.
2. The poll loop polls each batch every `LLM_BATCH_POLL_SECS`. When one has
   ended it downloads `results_url` (JSONL) and matches results by
   `custom_id`, never by position. Each primary output that parses goes into
   an adversarial batch (`r-<n>` and `s-<n>`).
3. When that ends, each incident whose two verdict lists parse is combined
   and written by the synchronous path's own `finish_extraction` (same
   combine checks, same stale-text guard in `write_extraction`).

`errored`, `canceled`, `expired` and missing results drop that incident from
the run; the next sweep finds it again, because its stored hash still
doesn't match. An unusable success (refusal, truncated or unparseable JSON)
also feeds the per-text backoff, as on the synchronous path.

**Restarts resume.** Every in-flight batch is a row of
`enricher_llm_batches` (migration `20261010120000`): batch id, stage,
`model_version`, and the exact text each incident was submitted with (plus
the primary output in the adversarial stage). The poll loop works from the
table, so a restarted pod resumes polling instead of re-submitting. Moving
to the adversarial stage inserts the new row and deletes the old one in one
transaction, after the new batch exists. Known gaps, all bounded:

- A crash between creating a batch and recording it orphans that batch: it
  runs and is billed, nobody reads it, and its incidents go into a later
  batch.
- A batch whose `model_version` is no longer the service's (a model change
  mid-run) is canceled and dropped.
- Batch mode records no churn measurement (the `churn` baseline isn't
  persisted).
- Batches belong to the Claude workspace, not to the key, so in-flight batch
  ids stay valid across a key rotation (or a future switch to keyless auth)
  in the same workspace.

**OpenAI batch.** The enricher has no OpenAI Batch API path, and
`LLM_SWEEP_MODE=batch` with `openai` is refused. The design leaves room for
one (the table has a `provider` column, the stage planning in `batch.rs` is
provider-neutral, and `custom_id` matching is the same idea), but OpenAI's
flow is different enough (upload a JSONL file, create a batch on it, download
an output file) to need its own client. **Recommendation: don't build it
yet.** On gpt-6-luna the whole workload is about $4.50 a month
(docs/enricher-openai.md), and the sweep is a small share of it outside
re-extractions; half of that is cents. Revisit only if re-extractions become
frequent or large.

## Prompt caching

**Verdict: use it, with the 1-hour TTL, on the primary call's system
prompt. On by default (`LLM_PROMPT_CACHE=1h`).**

### The prompt structure

Measured from `llm.rs` (characters, and tokens at 4 characters per token;
the current tokenizer may count up to ~1.35x that):

| Part | Static? | Size |
| --- | --- | --- |
| Primary system prompt (`PRIMARY_PROMPT`) | yes | 9,574 chars, ~2,400 tokens |
| Primary schema | yes | 1,216 chars, ~300 tokens |
| Resolution-adversarial system prompt | yes | 988 chars, ~250 tokens |
| Severity-adversarial system prompt | yes | 943 chars, ~240 tokens |
| Adversarial schemas | yes | ~410-450 chars, ~100 tokens each |
| Primary user content: reference date + summary + description | no | typically ~500 tokens |
| Adversarial user content: the text + the period list | no | ~650 tokens |

So about 80% of a primary call's input is a fixed prefix, while an
adversarial call is mostly per-incident. Only the system prompt carries the
`cache_control` marker; the schema travels in `output_config.format`, which
the API compiles into a grammar with its own 24-hour cache, and changing it
invalidates the prompt cache (it never changes between calls of one kind).

### Minimum cacheable length

A prefix shorter than the model's minimum silently doesn't cache (no error,
`cache_creation_input_tokens: 0`):

| Model | Minimum | Primary (~2,400) | Adversarial (~250) |
| --- | --- | --- | --- |
| Haiku 5.5, Sonnet 5.5, Opus 5.5 | 512 | caches | doesn't |
| Sonnet 5 / 4.6, Opus 4.8 | 1,024 | caches | doesn't |
| Haiku 4.5 | 4,096 | **doesn't** | doesn't |

So only the primary call benefits. The adversarial calls carry the marker
anyway: it costs nothing below the minimum and starts working if a prompt
grows past it. `prompt_sizes_match_the_caching_analysis` fails if a prompt
edit moves either side of the line. (Haiku 4.5 never caching this prompt is
one more reason the default is Haiku 5.5.)

### Pricing and break-even

Cache writes cost 1.25x the input price (5-minute TTL) or 2x (1-hour TTL);
reads cost 0.1x (Haiku 5.5: $0.01 per million tokens) or 0.05x (Sonnet 5.5
and Opus 5.5). With *h* the share of primary calls that find the prefix still
cached, the prefix's cost relative to no caching is
(1 − *h*) × write + *h* × read, so caching pays when:

- 5-minute TTL: *h* > 0.25 / (1.25 − read) ≈ **21%**;
- 1-hour TTL: *h* > 1 / (2 − read) ≈ **52%**.

### The enricher's request rate

From the code and the existing estimate (docs/enricher-openai.md, "Cost"):
about 200 extractions a day, i.e. about 200 primary calls a day, ~8 an hour
on average, in daytime clusters (a poll of the feed publishes several
changed incidents at once; the hourly sweep runs its finds back to back).
Measure the real rate with
`sum(increase(distant_signal_enricher_llm_call_total{call="primary"}[1d]))`.

- 5-minute TTL: at ~8 calls an hour spread evenly, the chance that the
  previous primary call was within 5 minutes is 1 − e^(−8.3/12) ≈ **50%**.
  Clustering raises it, quiet hours lower it.
- 1-hour TTL: a miss needs an hour with no primary call at all; that is a
  few times a day (overnight), so *h* ≈ **95%**.

Expected cost of the 2,400-token prefix relative to no caching:

| | No caching | 5m, *h* = 50% | 1h, *h* = 95% |
| --- | --- | --- | --- |
| Haiku 5.5 | 1.00 | 0.68 | **0.20** |
| Sonnet 5.5 | 1.00 | 0.65 | **0.15** |

In dollars (200 primary calls a day, synchronous):

| | Prefix input, no caching | With 1h caching | Saved per month |
| --- | --- | --- | --- |
| Haiku 5.5 | $0.048/day | ~$0.010/day | ~$1.15 (about 15% of the whole Haiku bill) |
| Sonnet 5.5 | $0.96/day | ~$0.14/day | ~$25 |

The downside is bounded and small: if the traffic were far sparser than
assumed (under one primary call an hour), every call would pay the 2x write,
about +$0.00024 a call on Haiku 5.5. Hence 1 hour by default; `5m` is there
for a much busier deployment, `off` for debugging.

### Batches

Caching works inside Message Batches, and the 50% batch discount stacks with
it (a Haiku 5.5 cache read in a batch is $0.005 per million tokens). A batch
runs its requests concurrently and can take longer than 5 minutes, so the
1-hour TTL (the default) is also what the Claude docs recommend for batches.
A batch of N incidents should then pay roughly one or a few cache writes and
N reads of the primary prefix. Cache hits in a batch are best effort.

### Watching it

`cached` and `cache_write` in the token counter (below). The primary call's
read ratio over a day:

```promql
sum(increase(distant_signal_enricher_llm_tokens_total{call="primary", kind="cached"}[1d]))
/
(
    sum(increase(distant_signal_enricher_llm_tokens_total{call="primary", kind="cached"}[1d]))
  + sum(increase(distant_signal_enricher_llm_tokens_total{call="primary", kind="cache_write"}[1d]))
)
```

Near 1 means the prefix is read from cache; near 0 with a nonzero
`cache_write` means the traffic is too sparse for the TTL; both 0 means the
prefix isn't cached at all (a model whose minimum is above the prompt).

## Model

**Default: `claude-haiku-5-5`** (Claude Haiku 5.5, released 2026-10-07).
Configurable with `enricher.llm.anthropic.model` / `LLM_MODEL`.

| Model | Input / output per MTok | Cache read | Notes |
| --- | --- | --- | --- |
| `claude-haiku-5-5` | $0.10 / $0.50 (prompts ≤ 100K tokens) | $0.01 | positioned for high-volume extraction and classification; adaptive thinking, default effort `medium`; 512-token cache minimum |
| `claude-sonnet-5-5` | $2 / $10 | $0.10 | 20x Haiku 5.5; default effort `high` |
| `claude-opus-5-5` | $4 / $20 | $0.20 | far more than this task needs |
| `claude-haiku-4-5` (`claude-haiku-4-5-20251001`) | $1 / $5 | $0.10 | previous Haiku: 10x Haiku 5.5's price, and its 4,096-token cache minimum means the primary prompt never caches |

Why Haiku 5.5: the job is structured extraction from short public text, the
kind of workload the Claude docs name for it, and it costs the same as the
OpenAI candidate (gpt-6-luna at $0.10/$0.50), so the two can be compared on
quality alone. Sonnet 5.5 is the step-up if the eval shows Haiku 5.5 missing
the hard cases (multi-leg segmentation, the BST date arithmetic): at ~$120-150
a month it is affordable, but 20x the price should be earned in the eval
first. The model the brief listed, `claude-haiku-4-5-20251001`, is the
previous Haiku and the worst value here (above).

Thinking: every current Claude model thinks adaptively by default, and its
tokens are billed as output (`completion` in the metrics; the API doesn't
report them separately). For a cheaper, faster run, measure `reasoning_effort
= "low"` and, on Haiku 5.5, `thinking = "disabled"` as extra eval targets.

## Metrics and cost

Unchanged metrics keep their meaning (`enricher_llm_call_total`,
`enricher_llm_call_duration_seconds`, `enricher_llm_model_info`, which
names the model and `base_url_host` `api.anthropic.com`). Changed or new:

- `distant_signal_enricher_llm_tokens_total{call, kind}` gains a fifth kind,
  `cache_write` (15 series, all registered at 0). For the Claude API:
  `prompt` is every input token (`input_tokens` +
  `cache_creation_input_tokens` + `cache_read_input_tokens`), `cached` the
  cache reads, `cache_write` the cache writes, `completion` `output_tokens`
  (thinking included), `reasoning` always 0. OpenAI never sends
  `cache_write`.
- Batch mode only (registered when `LLM_SWEEP_MODE=batch`):
  - `distant_signal_enricher_llm_batch_tokens_total{call, kind}`: the same
    kinds for batch results, kept apart because they are billed at half
    price;
  - `distant_signal_enricher_llm_batches_total{stage, event}`: `submitted`,
    `submit_failed`, `ended`, `poll_failed`, `abandoned`;
  - `distant_signal_enricher_llm_batch_requests_total{call, result}`:
    `succeeded`, `invalid`, `errored`, `canceled`, `expired`, `missing`;
  - `distant_signal_enricher_llm_batches_in_flight`.

There is no alert on batch failures yet: watch `abandoned`/`submit_failed`
and `errored`/`expired` after enabling batch mode.

Spend for Haiku 5.5 over the last day (1-hour cache writes at $0.20; use
$0.125 for `LLM_PROMPT_CACHE=5m`), in USD:

```promql
(
    (
        sum(increase(distant_signal_enricher_llm_tokens_total{kind="prompt"}[1d]))
      - sum(increase(distant_signal_enricher_llm_tokens_total{kind="cached"}[1d]))
      - sum(increase(distant_signal_enricher_llm_tokens_total{kind="cache_write"}[1d]))
    ) * 0.10
  + sum(increase(distant_signal_enricher_llm_tokens_total{kind="cached"}[1d])) * 0.01
  + sum(increase(distant_signal_enricher_llm_tokens_total{kind="cache_write"}[1d])) * 0.20
  + sum(increase(distant_signal_enricher_llm_tokens_total{kind="completion"}[1d])) * 0.50
) / 1e6
and on() count(distant_signal_enricher_llm_model_info{model="claude-haiku-5-5"})
```

Add the same expression over `enricher_llm_batch_tokens_total`, times 0.5,
for batch mode. The Claude Console's usage page stays the source of truth.

### Cost

Assumptions as in docs/enricher-openai.md: ~200 extractions a day, ~5,000
input tokens each (2,400 of them the cacheable primary prefix), plus output.
Output is the uncertain part: ~450 visible tokens per incident plus adaptive
thinking, assumed here at ~1,000 more (measure it in the eval records).

| | Input/day | Output/day | Per month |
| --- | --- | --- | --- |
| Haiku 5.5, 1h caching | ~$0.06 | ~$0.15 | **~$6-8** |
| Sonnet 5.5, 1h caching | ~$1.1 | ~$3.0 | **~$120-150** |

Switching provider or model re-extracts every live incident once
(`model_version` is `<model>@periods-v2`): a few hundred incidents, cents on
Haiku 5.5, and half that with batch mode. Set the workspace spend limit at
about twice the expected month.

## Switching and rolling back

Switch: set the chart values in [Setup](#setup). Expect one re-extraction
pass on the next sweep (synchronous, or batched with
`enricher.llm.batch.sweepMode: batch`, which finishes within hours instead
of in one long sweep). Watch `enricher_llm_call_total` by outcome,
`enricher_llm_call_duration_seconds`, and the cache read ratio above.

Roll back: restore the previous `enricher.llm.*` values (or just
`provider: openai`). `model_version` changes back, so incidents re-extract
with the previous model. In-flight batches are then canceled and dropped by
the next poll (their `model_version` no longer matches), or, if batch mode is
off, left in `enricher_llm_batches` unread; delete those rows by hand
(`DELETE FROM enricher_llm_batches`) once you are sure you won't switch back.

## Evaluating before switching

The model-eval harness (docs/enricher-model-eval.md) runs the service's own
client, so it runs the Claude API with the same requests:

- `crates/enricher/eval/targets.example.toml` has
  `anthropic-claude-haiku-5-5` and `anthropic-claude-sonnet-5-5` targets
  (`provider = "anthropic"`, key from `ANTHROPIC_API_KEY`), next to the
  current ones; or set `LLM_PROVIDER=anthropic` and `LLM_API_KEY` for a
  single env-var target. The live evals and `replay_eval` read the same
  variables.
- Look for: a 100% valid-output rate (no `refused`, no HTTP 400, which would
  mean a schema the API rejects), no false `resolved`, segmentation at least
  as good as the current model's, and consistent repeats (there is no
  `temperature: 0` here).
- Check the token records: `cache_write_tokens` on the first primary call,
  `cached_tokens` on the following ones, and how many output tokens thinking
  adds.

## Data handling

What leaves the cluster: the incident summary and description (public
National Rail Knowledgebase text), the reference date and the prompts; no
user data. Per Anthropic's data retention page (read 2026-10-09; re-check
before switching), API data is not used for model training without
permission, and Message Batches are stored for 29 days by design (outside
zero data retention). The privacy notice and the UK legal review assume a
self-hosted model, so the same follow-up as for OpenAI applies
(docs/enricher-openai.md, "Legal follow-up before switching"), with Anthropic
as the processor.

## Keyless auth (not implemented)

The Claude API supports workload identity federation, but the enricher only
takes an API key with `anthropic` for now. The seam is `auth.rs`'s
`Credential`: a federated token source would mint a token
(`POST {base}/oauth/token`, RFC 7523 `jwt-bearer` grant) that goes out as
`Authorization: Bearer` with no `x-api-key`, with no change to `llm.rs`. The
subject JWT is single-use, so that source must re-read the projected token
for every exchange.
