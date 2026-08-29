# Project Naming Research

**Status: research/brainstorming pass only — not a decision.** This
document surveys what the project has become, proposes candidate names
with real (if informal) conflict-checking, and sizes the footprint of an
actual rename. Whether to rename at all, and which name to pick if so, is
the project owner's call — nothing here is committed, and nothing in the
codebase, repo metadata, or configuration was touched to produce it.

## 1. What this project actually is now

**One-line characterization:** what started as a TfL-style line-status
aggregator for National Rail has grown into a personal UK rail companion —
network status, individual train tracking, accounts, and (soon) ticket/
Delay-Repay support — built by and for people who care about the detail
UK rail data actually contains, not a commercial booking or journey-planning
product.

That's not just README framing — it's what a survey of the actual repo
shows:

| Capability | State | Evidence |
|---|---|---|
| Line-status aggregation (original core) | Implemented, still central | `DESIGN.md`, `crates/aggregator`, `lines/*.toml` |
| Individual train journey tracking (TRUST-sourced) | **Implemented, merged** | `crates/trust-consumer` (10 source files), `git log`: `d9811d3 Merge branch 'worktree-train-tracking'`, `6085302 Wire the full TRUST consume-match-derive-write processing loop` |
| User accounts (OIDC/SSO) | **Implemented, merged** | `crates/api/src/auth/oidc.rs`, `crates/api/src/auth.rs` (`pub mod oidc;`), design doc `2026-08-28-user-accounts-sso-design.md` |
| Per-service Prometheus metrics | **Implemented, merged** | `crates/common/src/metrics.rs` (`nr_status_` prefix helper), `git log`: `56d8e9f Merge branch 'worktree-metrics-impl'`, metrics wired into every crate |
| Local dev identity provider (Authentik, opt-in) | **Implemented, merged** | `git log`: `4de1e3b Add opt-in docker-compose.authentik.yml local dev IdP overlay`, `38700ae Add the devAuthentik values.yaml block` |
| Journey ticket tracking + Delay Repay eligibility estimation | **Design only, not implemented** | `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md` — explicitly never auto-submits a claim, only estimates and links out; no barcode/ITSO decoding |
| Line-catalogue expansion to all major TOCs/regional networks | **Research/gap-analysis only, not implemented** | `docs/superpowers/specs/2026-08-29-line-coverage-gap-analysis.md` (766 lines; audit of what's missing, no new `.toml` files written) |

Two things worth being honest about rather than taking the brief's framing
at face value:

- **`README.md`/`DESIGN.md` are themselves stale**, describing a
  single-package Python demo with no HTTP layer, no persistence, and
  train-level tracking explicitly out of scope (`DESIGN.md` §2). The
  actual codebase is a nine-crate Rust workspace
  (`common`, `api`, `poller-incidents`, `poller-stations`, `poller-tocs`,
  `poller-ldbws`, `poller-tfl`, `aggregator`, `enricher`, `trust-consumer`)
  plus a Next.js frontend and a Helm chart — none of which `README.md`/
  `DESIGN.md` mention at all. This is itself evidence the docs need
  attention regardless of what happens with the name.
- **The scope growth is real, not aspirational.** Four of the six new
  capabilities above are merged and running, not just planned. Train
  tracking in particular is a genuine architectural departure (the
  design doc calls it "the single most architecturally significant
  finding": a persistent Kafka consumer, the first long-running
  stream-consumer service in the stack alongside `enricher`) — this
  isn't scope creep in name only, it changed what kind of system this is.

So the honest target for naming purposes is: **a personal rail companion
whose core is still live network status, with individual train tracking,
accounts, and (soon) ticket/compensation help layered on** — not "a line
status board" and not yet "a full journey-planning/booking app" (no
timetabling, no fares/booking, no barcode/ITSO ticket handling — those
were explicitly investigated and rejected for the ticket-tracking feature).

## 2. Existing UK rail app landscape (context for naming, not just conflict-checking)

Before generating candidates, it's worth noting what this project is now
walking into, competitively and stylistically. A web search for
UK rail status apps turned up an unexpectedly close analog:
**"Trackd – Live GB Rail Status"** (iOS) — live network status across all
operators, station facility info, *and* integrated Delay Repay guidance
with links to claim forms. That's close to this project's current +
planned scope. Other real, currently-live UK rail apps found during
research: TrainTrack UK, Trainy, TrainMapper, Raildar, Railboard,
Railwise, RailO, Train Beacon, Station Master (Geoff Marshall's TfL/Tube
reference app, not National Rail), and the official National Rail
Enquiries app. None of these compete on this project's actual
differentiators (open-source, self-hosted, curated shared-trunk line
modelling, TRUST-sourced individual tracking, dataQuality provenance) —
but their existence is a reason to make sure a chosen name doesn't sound
like a minor variant of one of them.

House style across this landscape is genuinely plain and functional:
"National Rail Enquiries," "Realtime Trains," "Trainline," "Railboard,"
"Train Beacon." A few hobbyist/enthusiast entries lean drier or cheekier
("Delay Repay Sniper"). Nothing in the space uses a startup-abstract name
(no "Wanderly," no "Transito") — see §4 for what this implies for
candidate selection.

## 3. Candidate names

Ten candidates, each with reasoning, a tone/fit gut-check, and a real
conflict-check finding from web search (not just wordplay — see method
note below each cluster).

### Clear or near-clear

**1. Distant Signal**
- *Reasoning:* A "distant signal" is a real UK signalling term — a signal
  placed in advance of the main stop signal, giving early warning of what's
  ahead. Strong metaphor for this project's actual shape: aggregate status
  (advance warning of disruption), individual train tracking (watching one
  train's approach), and the ticket-tracking feature's Delay Repay
  eligibility estimate (an early warning about compensation, not a
  guarantee). Reads as insider/enthusiast vocabulary, which fits a project
  built around curated shared-trunk line modelling and TRUST message-type
  research.
- *Tone/fit gut-check:* Fits the dry, plain-vocabulary British-rail
  register well; risks being opaque to anyone who isn't already
  rail-literate, which for this project's actual audience (people who'll
  read a segment-registry README) is probably fine, not a cost.
- *Conflict check:* Clear. No UK rail app, well-known open-source project,
  or product by this name found. (A couple of unrelated model-railway
  blogs use the phrase generically; not a real conflict.)

**2. Home Signal**
- *Reasoning:* Another real signalling term — the signal that protects a
  station or junction and gives the "proceed" authority for that specific
  point. Reasonable metaphor for "here's the definitive status for this
  line/train," slightly narrower fit than Distant Signal since it's about
  one point rather than advance/aggregate warning.
- *Tone/fit gut-check:* Same register as Distant Signal; same
  insider-vocabulary trade-off.
- *Conflict check:* Near-clear. `signalbox.org` ("The Signal Box," a UK
  railway-signalling history site) uses "Home Signal" as a tongue-in-cheek
  label for its own homepage — not a product, not a naming conflict in any
  formal sense, but worth knowing the phrase is already loosely associated
  with UK rail-signalling enthusiast content elsewhere online.

**3. Trackside**
- *Reasoning:* Plain, immediately legible even to non-enthusiasts, evokes
  "watching from beside the tracks" — fits both the status-board and
  individual-train-tracking angles without over-committing to either.
  Versatile as a brand ("your trackside companion").
- *Tone/fit gut-check:* Good middle ground — plainer than Distant
  Signal/Home Signal, still rail-specific rather than generic-tech.
- *Conflict check:* Clear. No UK rail app or well-known open-source
  project found under this name (searches surfaced only unrelated
  motorsport/model-railway content and generic GitHub repos with no
  overlapping name).

### Soft conflicts (real, but not disqualifying)

**4. Calling Points**
- *Reasoning:* "Calling points" is the real industry/passenger term for
  the stations a service stops at — a natural fit for a project whose
  newest core feature is *literally* walking a train's calling points from
  TRUST movement events (see `2026-08-28-train-tracking-design.md`
  §"Position-in-journey derivation").
- *Tone/fit gut-check:* Reads as authentic rail vocabulary, not marketing
  spin — good fit for the register.
- *Conflict check:* Soft conflict. Not used as a product name by any
  single dedicated app, but it's already extremely common as *feature
  copy* across nearly every existing UK rail app found in this research
  (UK Live Train Times, RailO, Railboard, National Rail Enquiries itself
  all use "calling points" to describe this exact feature). That
  genericness is a real weakness for a proper-noun brand name even though
  it isn't a trademark/product conflict — it may read as a generic label
  rather than a distinctive name.

**5. TrainTrace**
- *Reasoning:* Direct compound of "train" + "trace," describing the
  event-log/journey-history behavior the train-tracking design actually
  builds (an immutable TRUST event log per tracked train).
- *Tone/fit gut-check:* Slightly more generic-tech-sounding than the
  signalling-term candidates; still legible and on-topic.
- *Conflict check:* Soft conflict. No exact match found, but "Train Track"
  (a real, multi-repo commercial-style journey-tracking app,
  `traintrackapp.co.uk`, with Android/iOS/web/Firebase repos on GitHub) is
  phonetically and conceptually close enough that "TrainTrace" risks being
  heard/misremembered as a variant of it.

**6. Trackwise**
- *Reasoning:* "Wise about the tracks" — plays on the "-wise" suffix
  pattern that's already common in this space.
- *Tone/fit gut-check:* Sounds plausible for the space, which is itself
  part of the problem (see conflict below) — it's *too* on-pattern.
- *Conflict check:* Soft-to-real conflict, two ways. (a) **Railwise – Your
  Train Tracker** is a real, currently-listed iOS train-tracking app —
  near-identical naming pattern in the exact same product category.
  (b) **Trackwise Designs plc** (now Amphenol Trackwise Designs Ltd) is a
  real, formerly publicly-listed UK company (PCB manufacturing, unrelated
  industry, but a known UK corporate name with King's Awards recognition)
  — a second, unrelated but real prior claim on the word. Two independent
  soft conflicts on one candidate is enough to deprioritize it.

### Real/hard conflicts (recommend avoiding)

**7. Signal Box**
- *Reasoning:* The building that actually controls signals and points —
  a strong literal metaphor for "the thing that decides the aggregate
  status."
- *Tone/fit gut-check:* Excellent register fit if the name were free.
- *Conflict check:* Real conflict. `signalbox.org` ("The Signal Box") is
  an established, long-running UK railway-signalling enthusiast resource
  with substantial content, in the *exact* topical neighborhood (rail
  signalling detail, enthusiast audience) this project's own README/DESIGN
  register is aimed at. Not a legal trademark issue, but a real risk of
  audience confusion with a well-known resource in the same niche.

**8. RailPulse**
- *Reasoning:* Evokes real-time/heartbeat data — sounds like a plausible
  live-status product name.
- *Tone/fit gut-check:* Reads more generic-tech than the UK rail register,
  already a mark against it.
- *Conflict check:* Hard conflict. **RailPulse** is a real, well-established
  North American freight-rail industry consortium/telematics platform
  (founding members include GATX, Norfolk Southern, Trinity Rail,
  Greenbrier — an industry joint venture, not a hobby project), actively
  operating and referenced in trade press as of 2026. Clear conflict;
  avoid.

**9. Concourse**
- *Reasoning:* A station concourse is a natural gathering-point metaphor
  for a project that aggregates status, tracking, and (soon) tickets in
  one place.
- *Tone/fit gut-check:* Reasonable metaphor, more architecture-adjacent
  than rail-specific in feel.
- *Conflict check:* Hard conflict. **Concourse CI** (`concourse-ci.org`,
  `github.com/concourse/concourse`) is a well-known, actively maintained
  open-source CI/CD project with real name recognition in exactly the kind
  of developer/open-source audience this project's own contributors come
  from. Using "Concourse" for an unrelated open-source project in the same
  general audience is a genuine, avoidable collision.

**10. StationMaster**
- *Reasoning:* A real, historic UK rail job title — immediately readable,
  evokes authority/oversight over a station's operation.
- *Tone/fit gut-check:* Good register fit if free.
- *Conflict check:* Real conflict. **Station Master** (by Geoff Marshall,
  a well-known UK transport figure) is an established, award-recognized
  iOS reference app for the London Underground/Overground/DLR network —
  praised by Time Out and the Evening Standard, won a TfL accessibility
  award. Different data domain (Tube vs. National Rail) but close enough
  in name, audience, and "detailed rail reference app by an enthusiast for
  enthusiasts" positioning that reuse would invite real confusion. There's
  also an unrelated mobile game of the same name. Avoid.

## 4. Top recommendations (ranked)

1. **Distant Signal** — clearest conflict result of all ten, the metaphor
   maps onto more of the project's actual current shape (aggregate status
   as advance warning, individual-train watching, Delay-Repay estimation
   as an early, non-binding heads-up) than any other candidate, and it
   commits to the insider-vocabulary register this repo's own
   documentation already uses without hedging (`DESIGN.md` casually
   assumes the reader knows what a "shared trunk" or "junction station"
   is). For a hobby open-source project, leaning into that register beats
   softening toward something more generically approachable — the
   audience self-selects already.
2. **Trackside** — best runner-up specifically because it's *less*
   cryptic than the signalling-term names while staying rail-specific and
   equally conflict-clear. If the owner wants something legible to a
   general audience (not just rail enthusiasts) on first read, this is the
   safer choice with almost no downside versus Distant Signal.
3. **Home Signal** — a close third to Distant Signal on the same
   reasoning, marginally narrower metaphor (one point vs. advance warning
   across the whole picture) and one loose, non-blocking prior use
   (`signalbox.org`'s homepage joke) worth being aware of but not
   disqualifying.
4. **Calling Points** — worth a place in the top group on merit (the most
   literal, feature-accurate name of the ten, and genuinely descriptive of
   the train-tracking core), but ranked below the top three specifically
   because of its genericness as *existing feature copy* across the whole
   competitive landscape — it risks reading as a description rather than
   a name.
5. *(No fifth strong recommendation.)* TrainTrace and Trackwise both carry
   real, if soft, naming-adjacency risk against existing named products in
   the same category; Signal Box, RailPulse, Concourse, and StationMaster
   all have real conflicts and are not recommended regardless of ranking.

## 5. Rename footprint survey

A quick, targeted grep (excluding `.git`, `node_modules`, `target`) found:

- **1,203 occurrences of `nr-status`**, but the overwhelming majority
  (1,190) are inside `docs/` (historical plan/spec prose — append-only
  project history, not living config) and `charts/nr-status/` (24 files,
  almost all just the Helm *directory path* repeated per template file,
  not 24 independent things to edit).
- **122 occurrences of `nr_status`** (the underscore variant used where a
  literal identifier is needed — env vars, metric names), similarly
  concentrated in `docs/` and a small number of real config/code sites.

Filtering out `docs/` and `plans/` (historical record, not live
config/code) leaves the actual footprint of a real rename:

| Item | What it is | Rename effort |
|---|---|---|
| `README.md`, `DESIGN.md` | Project title/self-description | Direct text edit — needs doing either way, since both are already stale on architecture (see §1) regardless of naming. |
| `charts/nr-status/` (directory) + `Chart.yaml` `name:` field | Helm chart name | Rename directory, update `Chart.yaml`; existing installs would need a `helm uninstall`/reinstall or a chart-rename migration path (Helm does not support renaming a release's chart in place cleanly) — an actual operational consideration for anyone with a live deployment, not just a text change. |
| `charts/nr-status/templates/*.yaml` (23 files) | Reference the chart name via Helm's templating (`_helpers.tpl`), not individually hardcoded per file | Effectively free once `Chart.yaml`/`_helpers.tpl` are updated — these aren't 23 separate edits. |
| `docker-compose.yml`, `dev.env.example`, `local.env.example` | `POSTGRES_USER=nr_status` / `POSTGRES_DB=nr_status` env defaults | Cosmetic env-var value changes; only matters for anyone with a live local Postgres volume already using that db/user name (a real but small migration step for existing dev environments). |
| `crates/common/src/metrics.rs` | `nr_status_` prefix applied to every hand-emitted Prometheus metric name | **The one genuinely load-bearing rename item.** Every metric this app emits is namespaced `nr_status_*` (`crates/api/src/main.rs`'s `.with_prefix("nr_status")`, plus per-crate metrics referencing the same prefix in comments/tests). Renaming this breaks any existing Grafana dashboard, alert rule, or PromQL query built against the old prefix — a real breaking change for anyone already running this in production, not just a rename. |
| `crates/trust-consumer/src/config.rs` | Kafka consumer-group-id default (`nr-status-trust-consumer`) | Renaming a Kafka consumer group ID resets that group's committed offsets on next connect — an operational, not just cosmetic, change for anyone with a live TRUST consumer already running. |
| `crates/api/migrations/20260510023522_initial.sql` | One historical comment line (`-- nr-status-v2 database schema`) | Should **not** be edited — migrations are append-only history in this repo's own convention; leave as-is regardless of any rename. |
| GitHub repo name (`FasterSpeeding/Network-Rail-Status`) | Repo/org path | Out of scope for this research pass per the brief, but the obvious top-level item any real rename would also touch — GitHub redirects old URLs automatically, so this is lower-risk than it sounds. |
| Crate names (`crates/common`, `crates/api`, `crates/aggregator`, `crates/enricher`, `crates/poller-*`, `crates/trust-consumer`) | Rust workspace member names | **Not** `nr-status`-branded at all today — already generic/functional names. A rename would touch **zero** crate names. |
| `lines/` catalogue | TOML line definitions | Checked — no internal `nr-status`/`nr_status` self-references found anywhere in `lines/`. A rename touches nothing here. |

**Bottom line on footprint: smaller than 1,203 occurrences suggests, but
not zero-cost.** The real touch points are: two doc files, one Helm chart
name (with a real deploy-migration wrinkle for existing installs), three
env-var defaults (cosmetic unless a live dev DB already exists), one
Prometheus metric-name prefix (the one genuinely breaking change for any
live deployment with dashboards/alerts), and one Kafka consumer-group ID
(offset-reset risk for a live TRUST consumer). Crate names and the `lines/`
catalogue need no changes at all. This is a half-day-scale mechanical
change for a project with no production deployment yet watching metrics —
materially larger and riskier for one that does.

## 6. Honest recommendation: is renaming worth it right now?

**Lean no — update `README.md`/`DESIGN.md`'s self-description instead,
and revisit the name later if it still bothers the owner once the docs
are current.** Reasoning:

- **The docs are stale on architecture, not just on name.** `README.md`
  and `DESIGN.md` currently describe a single-package Python demo with no
  HTTP layer and train-tracking explicitly out of scope — that's a bigger,
  more urgent honesty gap than the project's *name* being narrower than
  its current scope. Fixing the self-description to reflect the real
  nine-crate Rust/Next.js/Postgres/Kafka system, with line-status,
  tracking, accounts, and metrics all described accurately, is valuable
  regardless of what the project is called, and is strictly
  lower-risk/lower-effort than a rename.
- **The rename's one real cost (the Prometheus metric prefix) only bites
  once there's a live deployment with dashboards/alerts to break.** If
  this is still pre-production, that cost is close to zero right now and
  will only grow the longer the project runs under the current name — so
  if a rename is ever going to happen, doing it *before* the metrics
  prefix and Kafka consumer group have real operational history behind
  them is the cheapest possible timing. That's an argument for "rename
  soon if at all," not "rename now regardless."
- **The name "nr-status" isn't actively misleading** the way a name
  implying official National Rail endorsement would be (Network Rail's own
  terms explicitly forbid exactly that framing, and this project's design
  docs already take that seriously — see the train-tracking design's
  unbranded-attribution discussion). It's under-scoped, not wrong. That's
  a much weaker case for urgency than a name that actively misrepresents
  what the project does or implies an affiliation it doesn't have.
- **If the owner does want to rename**, this research suggests doing the
  README/DESIGN.md accuracy pass *first* (cheap, no footprint, clarifies
  what the project actually is before naming it), then picking from the
  top 3 in §4 with that clearer self-description in hand — a name chosen
  against an accurate description is more likely to actually fit.

**If a decision is wanted despite the above: Distant Signal**, on the
strength of its conflict-clear result and the fit of its metaphor to the
project's real current shape (§4, item 1).
