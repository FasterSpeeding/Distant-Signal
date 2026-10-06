# Using the OpenAI API for the enricher

How to point the enricher (`crates/enricher`) at OpenAI's own platform with
`gpt-6-luna` at reasoning effort `none`, what that costs, and what to check
before switching production.

**Status (2026-10-06): not deployed.** The production default is still the
self-hosted model. Nothing in the chart or the service defaults to OpenAI.
Switch only after the [evaluation checklist](#before-switching-production)
passes.

OpenAI documentation this page relies on (read 2026-10):

- Structured outputs: <https://developers.openai.com/api/docs/guides/structured-outputs>
- Latest model guide (reasoning effort and `temperature`): <https://developers.openai.com/api/docs/guides/latest-model>
- Model page: <https://developers.openai.com/api/docs/models/gpt-6-luna>
- Rate limits: <https://developers.openai.com/api/docs/guides/rate-limits>
- Error codes: <https://developers.openai.com/api/docs/guides/error-codes>

## Configuration

Service env vars (the chart value that sets each one is in brackets):

| Env var | Value | Why |
| --- | --- | --- |
| `LLM_BASE_URL` | `https://api.openai.com/v1` | [`enricher.llm.baseUrl`] |
| `LLM_MODEL` | `gpt-6-luna` | [`enricher.llm.model`]. Also the extraction's `model_version`, so switching re-extracts every uncleared incident on the next sweep. |
| `LLM_API_KEY` | an OpenAI project key | [`enricher.llm.existingSecret` + `existingSecretApiKeyKey`]. Use a dedicated project with its own key and a monthly budget. |
| `LLM_REASONING_EFFORT` | `none` | [`enricher.llm.reasoningEffort`]. Required; see below. |
| `LLM_MAX_TOKENS` | **unset** | This model reportedly rejects `max_tokens`. Unset means the field is never sent. |
| `LLM_REQUEST_TIMEOUT_SECS` | `120` | [`enricher.llmRequestTimeoutSecs`]. Effort `none` answers in seconds; confirm with the perf benchmark. |
| `LLM_MAX_IN_FLIGHT` | `3` | [`enricher.extraEnv`] |
| `LLM_RATE_LIMIT_RETRIES` | `3` | [`enricher.extraEnv`]. Real rate limits only; a quota 429 is never retried (below). |
| `LLM_GATEWAY_RETRIES` | `1` | [`enricher.extraEnv`]. One retry on 502/503/504 or a client timeout. |

These are the same settings as the `openai-gpt-6-luna-none` target in
`crates/enricher/eval/targets.example.toml`, so the eval measures the
configuration you would deploy.

Chart values (the key lives in a Secret you create; never in values):

```yaml
enricher:
  llm:
    baseUrl: https://api.openai.com/v1
    model: gpt-6-luna
    reasoningEffort: "none"
    existingSecret: enricher-openai        # kubectl create secret generic enricher-openai --from-literal=llm-api-key=...
    existingSecretApiKeyKey: llm-api-key
  llmRequestTimeoutSecs: 120
  extraEnv:
    - { name: LLM_MAX_IN_FLIGHT, value: "3" }
    - { name: LLM_RATE_LIMIT_RETRIES, value: "3" }
    - { name: LLM_GATEWAY_RETRIES, value: "1" }
```

Quote `"none"` in YAML. Unquoted, it is still the string `none`, but
quoting makes it obvious it is not a null. If the chart's opt-in egress
NetworkPolicies are on, the enricher's public-internet egress already
covers `api.openai.com`. Any `extraEgress` rule kept for the old tailnet
endpoint can go once the switch is done.

### Why the effort must be `none`

Every request the enricher sends has `temperature: 0`, so the same text
gives the same extraction as far as the model allows. For gpt-6-luna,
`temperature` is accepted only at reasoning effort `none`. At every other
effort (`low`, `medium`, `high`, `xhigh`, `max`) it must be left out, and
leaving `LLM_REASONING_EFFORT` unset means the model's default, `medium`,
so an unset effort breaks every request (latest-model guide; model page).
The enricher has no switch to drop `temperature`, on purpose: effort `none`
is the configuration chosen. Effort `none` is also the cheapest and
fastest option, and it leaves no reasoning tokens to bill (check
`reasoning_tokens` in the eval records).

`LLM_REASONING_EFFORT=none` passes config validation (the knob is free
text) and is sent as `"reasoning_effort": "none"`. Both are unit-tested
(`config::tests::reasoning_effort_none_is_accepted_and_passed_through`,
`llm::tests::reasoning_effort_none_is_sent_with_temperature_and_without_max_tokens`).

### Schema strictness

The enricher always sends `response_format: {"type": "json_schema",
"json_schema": {"strict": true, ...}}`. In strict mode OpenAI requires
(structured-outputs guide):

- `additionalProperties: false` on every object;
- every property listed in `required`;
- a root that is an object;
- nullable written as a type union with `"null"`.

OpenAI rejects a schema outside the supported subset with an error rather
than ignoring it. The three schemas in `llm.rs` (primary,
resolution-adversarial and severity-adversarial) meet these rules. The
nullable objects `date_range` and `schedule_window` are written as
`anyOf: [{object}, {"type": "null"}]`, the form the guide documents for
optional objects. `llm::strict_schema_tests` walks every schema and fails
on any object without `additionalProperties: false`, any property missing
from `required`, a non-object root, a nullable object written as a type
union, or an unsupported keyword (`allOf`, `oneOf`, `not`,
`if`/`then`/`else`, `dependent*`, `patternProperties`,
`minLength`/`maxLength`, `$ref`). It also checks that the shipped eval
dataset's gold answers validate against the schemas and parse through the
service's own parsers.

The tightened schemas are also what Ollama and NVIDIA receive. The JSON the
model returns is the same shape, and the parsers did not change.

### Errors and what the enricher does with them

| Response | Outcome label (`enricher_llm_call_total`) | Retried in-call? | Feeds the per-text backoff? |
| --- | --- | --- | --- |
| 429 rate limit (`rate_limit_exceeded`, or no OpenAI body) | `rate_limited` | Yes, up to `LLM_RATE_LIMIT_RETRIES`, waiting `Retry-After` (at least `LLM_RATE_LIMIT_RETRY_SECS`; over 600 s fails at once) | No |
| 429 with `error.code`/`error.type` `insufficient_quota`, `billing_hard_limit_reached` or `billing_not_active` | `quota_exhausted` | No: no wait fixes an empty account | No: it isn't the text's fault. Reclaim keeps retrying at its normal cadence, and each attempt fails fast and costs nothing. |
| 503 `server_is_overloaded` (and any 502/504) | `gateway_error` | Yes, up to `LLM_GATEWAY_RETRIES`, after `Retry-After` when sent (same 600 s cap), otherwise 2 s, 4 s, ... up to 30 s | 502/503 no; 504 only when gateway retries are off |
| 200 with `message.refusal` set and null content (a safety refusal) | `refused` | No | Yes: it is the model's answer to this text |
| 200 with null/empty content | `empty_content` | No | Yes |

Every failed call is logged with the response's `x-request-id`
(`request_id` field), plus `error.code`/`error.type` from the body. Give
OpenAI support the `request_id` when you report a problem. In the model
eval, `refused` and `empty_content` count as invalid output, not transport
failures, and each record keeps the response's token `usage`
(`prompt_tokens`, `completion_tokens`, `reasoning_tokens`,
`cached_tokens`) when the provider sends it.

`DistantSignalEnricherErrors` fires on any non-`success` outcome. A
`quota_exhausted` outcome means "top up the account or raise the budget".

## Cost

Prices: about **$0.10 per million input tokens** and **$0.50 per million
output tokens** (model page, 2026-10; check before relying on it).

Assumptions:

- About 200 incident extractions a day, each making 3 calls (primary plus
  two adversarial passes): 600 calls a day.
- Input per incident: about 5,000 tokens. The primary system prompt and
  schema are about 2,700 tokens (measured from `llm.rs` at 4 characters per
  token); incident text and wrappers add about 500. Each adversarial call is
  about 900 tokens.
- Output per incident: about 450 tokens (a few hundred for the primary
  pass, under 100 for each verdict list). Effort `none` adds no reasoning
  tokens.
- No credit taken for prompt caching. The long primary prompt is a stable
  prefix, so `cached_tokens` may bring the cost down. The eval records show
  how much.

That gives about 1.0M input tokens ($0.10) and 0.09M output tokens
($0.05) a day: **about $4.50 a month**. Allowing for retries, text-edit
re-extractions and longer incidents, budget **$4–8 a month**. Set an
OpenAI project budget around $15 a month: the hard limit then surfaces as
`quota_exhausted` instead of an unbounded bill. Switching the model also
re-extracts every uncleared incident once (a one-off of a few cents per
hundred incidents).

**Tier 1 is enough.** The load is under one request a minute on average
and at most 3 in flight (`LLM_MAX_IN_FLIGHT`). That is far below Tier 1's
request and token limits (rate-limits guide), and 429s are still handled
if a burst after an outage hits them.

## Model snapshot

`gpt-6-luna` is a single, unpinned alias: OpenAI can update the model
behind it without a name change, and there is no dated snapshot to pin.
Extraction quality can therefore change with no deploy on our side.

- Re-run the quality eval (`openai-gpt-6-luna-none` target) every month,
  and whenever OpenAI announces a model update. Keep the records so the
  runs can be compared.
- `model_version` stays `gpt-6-luna`, so an upstream change does **not**
  trigger re-extraction. Only text changes do.

## Data handling

What leaves the cluster: the incident summary and description (public
National Rail Knowledgebase text, not personal data), the reference date,
and the prompts. No user data is sent.

Per OpenAI's API data commitments (<https://openai.com/enterprise-privacy/>,
read 2026-10; re-check before switching):

- API inputs and outputs are not used to train OpenAI's models by default.
- They are kept for up to 30 days for abuse monitoring, then deleted.
- Zero Data Retention is available only by arrangement with OpenAI sales,
  for eligible endpoints.
- The UK is not one of OpenAI's data-residency regions, so the data is
  processed outside the UK (by default in the US).

### Legal follow-up before switching

The UK legal review (`ds-review/uk-legal-compliance-2026-09-27.md`) and the
privacy notice (`frontend/app/privacy/page.tsx`: "we use a self-hosted AI
model") both assume the LLM is self-hosted. Switching makes OpenAI a US
processor of the incident text. Before the switch:

- Update the privacy notice: incident messages are sent to OpenAI (US) to
  extract timing and severity; no personal data is involved.
- Add OpenAI to the review's processors and international-transfers list.
  Accept OpenAI's DPA for the organisation, and check how it covers UK
  transfers (IDTA or the UK Addendum).
- Re-check that no incident free text carries personal data. The review
  rates this "very rarely". If one does, it now leaves the UK.
- The AI-transparency item (LEG-16) is unchanged in substance. Its wording
  should say "OpenAI" instead of "self-hosted".

## Before switching production

1. Create a dedicated OpenAI project with a key and a monthly budget
   (about $15), on Tier 1 or above.
2. Run the quality eval with the `openai-gpt-6-luna-none` target next to
   the current production target (see
   [enricher-model-eval.md](enricher-model-eval.md)). Look for:
   - a valid-output rate of 100%, with no `refused` and no schema rejection
     (an HTTP 400 on the first call would mean a schema problem);
   - no false `resolved`;
   - segmentation (`multi_period`) at least as good as the self-hosted
     model's;
   - consistent results across repeats.
3. Confirm that `temperature: 0` at effort `none` is accepted. The first
   eval call proves it: OpenAI rejects the request otherwise.
4. Run the perf benchmark from the cluster's network at concurrency 1 and
   3, with `request_timeout_secs = 120`. Expect a `fits` verdict.
5. Check token usage and cost in the records (`usage` on each call). Is
   `reasoning_tokens` 0? Is the cost in line with the estimate above?
6. Complete the legal follow-up above.
7. Switch with the chart values above. Watch `enricher_llm_call_total` by
   outcome and `enricher_llm_call_duration_seconds` for a day, and expect
   one re-extraction pass over uncleared incidents.
8. Rollback: restore the previous `enricher.llm.*` values. The model
   version changes back, so incidents re-extract with the self-hosted
   model.
