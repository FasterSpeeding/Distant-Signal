# Other UK Transit Networks — Research Follow-Up (Open Questions)

> **This plan produces no code changes and is not ready for implementation
> dispatch.** It is a prioritized list of further research steps only,
> gated on tooling/access this session may or may not have. Do not run this
> through `superpowers:executing-plans` or `superpowers:subagent-driven-development`
> expecting code tasks to come out the other end — there aren't any. The
> source document,
> `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`,
> already concluded "nothing surveyed here clears the bar for a genuine
> follow-up design-spec pass right now," and nothing below changes that.
> This document exists solely to make its "Open questions" section
> dispatchable, one item at a time, if and when it's worth spending more
> research time on this area at all.

## Goal

The source research flagged seven specific open questions that its own
tooling (exhausted web-search budget, several bot-walled/TLS-failing
sites, blocked `web.archive.org` access) left unresolved — and explicitly
called out that several of its "no API found" conclusions are **artifacts
of that session's tooling constraints, not settled facts**. This document
turns those seven into discrete, individually dispatchable research tasks:
what to check, what access/tool it needs, and whether a subagent with only
web/browser tools could realistically resolve it right now versus whether
it's blocked on something else (an operator's human reply, or this
session's own environment limitations).

Ordering follows the source document's own "Recommendation" priority
ranking, highest first: Translink/NI Railways (ranked "possible, not
recommended now" — the only network with confirmed working APIs) comes
first, then Manchester Metrolink ("watch, don't build" — real
infrastructure, currently closed), then Tyne and Wear Metro / West
Midlands Metro (low priority, inconclusive due to tooling blocks), then
Sheffield Supertram / Glasgow Subway (lowest priority, "not recommended in
any respect" — resolving these only matters if priorities change).
Edinburgh Trams and Nottingham Express Transit have no corresponding entry
in the source document's "Open questions" list (their negative results
weren't flagged as tooling artifacts), so there is nothing to dispatch for
them here.

**Dispatchability summary:** of the seven, five (open questions 1-4 and 7
below, i.e. Nexus, TfWM, SPT, Transport for South Yorkshire, and the NIR
TOC-codes check) look realistically resolvable by a subagent with only
`WebSearch`/`WebFetch`/browser tools, right now, in this or a similar
session. One (open question 5, Metrolink's historical API schema via
Wayback Machine) is uncertain — it depends on whether this session's
`web.archive.org` block is specific to the `WebFetch` tool or a deeper
network-level block; worth a quick probe via the Playwright browser tools
before writing it off. One (open question 6, Translink's actual API
schema) is flatly not dispatchable to any subagent — it requires a human
to send an email from a real address and wait for a reply.

---

## 1. Translink Transport Information API — actual field-level schema and NIR-vs-bus/Glider scope

*(Source doc open question 6; highest-priority network per the
Recommendation ranking.)*

**What to check:** Whether `translink.co.uk/api`'s "incident information"
data type actually covers NI Railways, or is scoped to bus/Glider only;
and the real field-level schema for all four documented data types
(journey plans, departure boards, bus stop data, incident information) —
specifically whether "incident information" looks Pattern-A-shaped
(ready-made aggregate status, like `poller-tfl`) or requires building
inference like the LDBWS pollers do.

**What it needs:** An API key, obtained by emailing
`servicedata@translink.co.uk` with a name, company (if any), and contact
email, per the public page. Translink's own page gives no indication of
turnaround time.

**Dispatchable now?** No. This is the clearest blocked item in the whole
list — it requires a human with a real, monitored email address to send
the request and then wait an unknown amount of time for a human reply
with the key, then actually exercise the API once granted. No subagent
can do the "send an email from a real identity and wait for a reply"
step. If the user wants this pursued, the email itself is a five-minute
task, but it's a human task, not a research-dispatch one — and even once
a key arrives, someone would need to spend a session actually calling the
API and writing up the schema.

## 2. NI Railways — genuinely and completely absent from RDM/Darwin/Knowledgebase coverage?

*(Source doc open question 7; same top-priority network — this is the
"does NIR actually sit outside GB National Rail's data ecosystem"
question that underpins whether NIR integration would need a whole new
reference-data catalogue.)*

**What to check:** Direct fetch of
`wiki.openraildata.com/index.php/TOC_Codes` (the page DESIGN.md itself
cites) to see whether NI Railways / Translink appears in the GB TOC codes
list at all. Also worth searching for any secondary source that quotes or
mirrors that page's content (GitHub repos that scrape Rail Delivery
Group/ATOC reference data often keep a copy of the TOC codes table), since
the primary page 403'd in the source research.

**What it needs:** A plain `WebFetch`/browser retry against
`wiki.openraildata.com` (the 403 may not be a durable bot-wall — MediaWiki
sites don't typically bot-wall as aggressively as commercial sites, so
it's worth simply retrying before assuming it's blocked), plus a
`WebSearch` pass for TOC-codes mirrors if the direct fetch still fails.

**Dispatchable now?** Yes. This only needs `WebSearch`/`WebFetch`
(possibly Playwright browser tools as a fallback if the wiki still
403's). No human-only step involved.

## 3. Manchester Metrolink — historical API schema via Wayback Machine

*(Source doc open question 5; second-priority network per the ranking —
real infrastructure existed, currently closed to new signups, but "if
TfGM's promised new solution ships with public registration, a fresh
check... might change this," so knowing what the old API returned is
useful context to have on hand.)*

**What to check:** `web.archive.org`'s snapshots of
`developer.tfgm.com`/the old TfGM Open Data Portal docs, to recover
whether the historical real-time Metrolink API exposed aggregate line
status (Pattern A, cheap) or only raw per-stop departures (Pattern B,
needs inference) — the same distinction that made DLR's integration more
expensive than a naive read of "TfL has an API" would suggest.

**What it needs:** Working `web.archive.org` access. The source research
states this was **blocked entirely in that session's environment** via
`WebFetch` — not a bot-wall on TfGM's part, an inability to reach Wayback
Machine at all from that tooling.

**Dispatchable now? Uncertain — worth a cheap probe first.** This
session has Playwright browser tools (`mcp__plugin_playwright__browser_navigate`
et al.) available, which the original research session may not have had.
Whether that changes anything depends on where the `web.archive.org`
block actually lives: if it's a restriction specific to the `WebFetch`
tool (a proxy/allowlist that tool goes through), a real browser
navigation might get through fine. If it's a network-level block for the
whole environment (e.g. a firewalled sandbox), Playwright will hit the
same wall. **Recommended first step for whoever dispatches this: have a
subagent try `browser_navigate` to a `web.archive.org` URL as a single
cheap probe before committing to the rest of the task.** If it fails,
this item is genuinely blocked on environment tooling, not on effort —
it needs either a human with an ordinary browser, or a differently
configured agent environment.

## 4. Nexus / Tyne and Wear Metro — real open-data presence, if any

*(Source doc open question 1; third-priority tier — "inconclusive, low
priority for a research re-check, not for design work," per the
Recommendation, but flagged as likely resolvable by "a human with an
ordinary browser... in well under an hour.")*

**What to check:** Whether Nexus (or a rebrand, e.g. `travelnortheast.uk`,
which also 403'd in the source pass) publishes any real-time Metro feed —
its own API, a Bus Open Data Service-adjacent product, or anything listed
under a developer/open-data section the source research's guessed paths
didn't find. Also worth searching for third-party consumers of a Nexus
API (transit-app developer forums, GitHub repos, npm/PyPI packages named
around "tyne wear metro api") as indirect evidence, since those wouldn't
require getting past `nexus.org.uk`'s own bot-wall at all.

**What it needs:** `WebSearch` primarily (a proper search pass — the
source research's search budget was exhausted before this network got a
turn), since the direct-fetch bot-wall (`nexus.org.uk` 403) may not be
avoidable even with a real browser (Radware and similar bot-detection
products often fingerprint automation, including headless/scripted
browsers, not just raw HTTP clients).

**Dispatchable now?** Yes, primarily via `WebSearch` rather than
`WebFetch`/browser navigation to `nexus.org.uk` directly — the search
route sidesteps the bot-wall entirely by relying on third-party or cached
pages instead of a direct fetch. A `browser_navigate` attempt is worth
trying too but shouldn't be assumed to succeed just because it's a real
browser.

## 5. `api.tfwm.org.uk` — actual scope and public availability

*(Source doc open question 2; same third-priority tier as Nexus — West
Midlands Metro is only 2 lines even fully built, so this is opportunistic
research, not urgent.)*

**What to check:** Whether `api.tfwm.org.uk` (confirmed live,
ITO-World-hosted per its TLS cert) documents West Midlands Metro tram
data specifically, versus being scoped to bus/roadworks/car-park data
only (ITO World's historical TfWM product line). Also worth retrying
`developer.tfwm.org.uk`, which failed with a TLS handshake error rather
than a clean 403/404 in the source pass — that specific failure mode
reads more like a transient network issue than a deliberate block, and is
worth a simple retry before concluding anything.

**What it needs:** `WebFetch`/browser retry of both hosts, plus
`WebSearch` for "tfwm api developer documentation," "West Midlands Metro
open data," and similar — ITO World may document its own TfWM contract
scope on itoworld.com even if TfWM's own subdomains don't.

**Dispatchable now?** Yes. No bot-wall confirmed here (just a 403 with no
docs page, and a TLS handshake failure that's plausibly transient) —
straightforward for a subagent with `WebSearch`/`WebFetch`.

## 6. Transport for South Yorkshire — current domain and open-data presence

*(Source doc open question 4; fourth/lowest-priority tier per the
Recommendation, alongside Glasgow Subway/Edinburgh/Nottingham — "not
recommended, in any respect," so this is the least urgent item on this
list, included for completeness rather than because it's likely to change
the recommendation.)*

**What to check:** What South Yorkshire's transport authority is actually
called and where it lives online today. `sypte.co.uk` no longer resolves,
`transportforsouthyorkshire.gov.uk` (guessed) also failed to resolve, and
`travelsouthyorkshire.com` (where `supertram.com` redirects) sits behind a
Radware bot-wall. This is a pure identification question — find the
authority's real current domain — before any question about open data can
even be asked.

**What it needs:** `WebSearch` for "Transport for South Yorkshire official
website," "South Yorkshire PTE rebrand," or similar — this doesn't require
fetching any of the three dead-end domains directly, just finding the
right one via search.

**Dispatchable now?** Yes, and cheaply — this is exactly the kind of
"what is the current name/URL of this organization" question `WebSearch`
is good at, with no bot-wall or blocked-tool dependency.

## 7. SPT — real open-data path for Glasgow Subway, if one exists

*(Source doc open question 3; same lowest-priority tier — Glasgow Subway
is a single line, the smallest network surveyed, so this only matters if
priorities change dramatically.)*

**What to check:** Whether `spt.co.uk` has an open-data or developer
section anywhere in its site that a guessed `/open-data/` path (which
404'd) didn't find — check the site's actual navigation/sitemap, footer
links, and `robots.txt`/`sitemap.xml` for anything data- or API-shaped,
plus a proper `WebSearch` pass (the source research didn't get a working
general search for this network before its budget ran out).

**What it needs:** `WebFetch`/browser navigation of `spt.co.uk` itself
(reachable — only the guessed subpath failed) plus `WebSearch`.

**Dispatchable now?** Yes. No blocking access issue was found for
`spt.co.uk` itself in the source research; this is a "search harder and
look at the real site structure" task, well within a subagent's reach.

---

## If dispatching anything from this list

Per the source document's own recommendation, none of this is urgent, and
resolving all seven still wouldn't automatically produce a design-spec-worthy
result — it would just replace "unresolved" with either a confirmed
absence (most likely outcome for the lower-priority items) or a genuine
new candidate worth a dedicated design-spec pass (most plausible only for
Translink, and only after the email round-trip in item 1 above).

If only dispatching one or two items right now with just web/browser
tools, items 2 (NIR/TOC-codes check), 4 (Nexus), 5 (TfWM), 6 (Transport
for South Yorkshire domain), and 7 (SPT) are all cheap, independent,
parallelizable subagent tasks. Item 3 (Metrolink/Wayback) is worth a
one-shot probe if picked up. Item 1 (Translink's actual schema) is the
one that would matter most if resolved favorably — it's the only network
the source research ranked as "possible" at all — but it's gated on a
human sending an email and waiting, not on research effort.
