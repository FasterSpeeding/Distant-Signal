# Dark Reader / `color-scheme` Signal — Research

**Status: research/survey only, not an approved design.** No code was
changed to produce this — `layout.tsx`, `globals.css`, and everything
else in `frontend/` are untouched. Written to answer one narrow question:
can this app tell force-dark browser extensions like Dark Reader that it
already has a real light/dark theme, so the extension backs off instead
of double-theming it?

## Problem

`frontend/components/ThemeToggle.tsx` and `frontend/app/layout.tsx:106,109`
already give this app a genuine native theme (Mantine's `ColorSchemeScript`
+ `MantineProvider`, cycling light/dark/auto, per
`docs/superpowers/specs/2026-07-12-dark-theme-design.md`). A user running
Dark Reader (or a similar force-dark/recolor extension) gets it applied on
top of that regardless of which theme the site itself is showing, which
typically produces a broken, doubly-dark-themed page. The question is
whether there's a standard, documented way to opt out of that.

## Method

Two questions, kept separate because they have different, independently
verified answers: (1) what the CSS `color-scheme` property/meta tag does
per the web platform standard, and (2) what Dark Reader specifically,
currently, does with it. Both were checked against primary sources (MDN;
Dark Reader's own GitHub issues/discussions/`CONTRIBUTING.md`) rather than
assumed from training knowledge, per WebFetch/WebSearch results below —
each claim below is cited to the fetch that produced it.

## Finding 1 — `color-scheme` is a native-UI-chrome signal, not a content signal

MDN's `color-scheme` CSS property page states it lets an element "indicate
which color schemes it can comfortably be rendered in," and that user
agents use it to adjust: "the color of the canvas surface, the default
colors of scrollbars and other interaction UI, the default colors of form
controls, and the default colors of other browser-provided UI, such as
'spellcheck' underlines" — explicitly **not** the page's own content:
"Component authors must use the `prefers-color-scheme` media feature to
support the color schemes on the rest of the elements."
(https://developer.mozilla.org/en-US/docs/Web/CSS/color-scheme)

The `<meta name="color-scheme">` HTML page confirms the meta tag is the
document-level equivalent of that same property ("indicates a suggested
color scheme that user agents should use for a page … works at the
document level in the same way that the CSS `color-scheme` property
specifies the preferred and accepted color schemes of individual
elements") and documents the valid values: `normal`, `light`, `dark`,
`light dark`/`dark light` (first value preferred, second acceptable), and
`only light`. It makes **no mention of browser extensions anywhere** —
this is purely a browser/native-UI mechanism per spec.
(https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/meta/name/color-scheme)

So per the platform standard alone, `color-scheme` does nothing to
extensions by design — it's scrollbars/form-controls/spellcheck squiggles,
full stop. Anything it does to stop Dark Reader specifically is Dark
Reader choosing to read that signal, not a platform guarantee.

## Finding 2 — this app already emits `color-scheme: light dark`, today, and it evidently isn't enough

`frontend/app/globals.css:1` does `@import '@mantine/core/styles.css';`.
Mantine's own docs show that stylesheet includes:

```css
:root {
  color-scheme: var(--mantine-color-scheme);
}
```

(https://mantine.dev/styles/global-styles/, confirmed via WebFetch) and a
Mantine GitHub issue about this exact injected rule shows the variable's
compiled default value is `light dark` — i.e. Mantine's base CSS already
declares `color-scheme: light dark` on `:root` unconditionally, as a
static default, not dynamically switched per the resolved
light/dark/auto choice
(https://github.com/mantinedev/mantine/issues/7569, confirmed via
WebFetch/WebSearch).

That means this app is **not starting from zero**: it already carries the
platform-standard `light dark` declaration Dark Reader's own open feature
requests describe as "the accepted way to tell automatic dark mode tools
to back off." That it evidently isn't backing off (per the premise of this
research) lines up with what Dark Reader's own bug tracker shows in
Finding 3 below — `light dark` specifically has a documented history of
not being reliably respected.

No `<meta name="color-scheme">` or `<meta name="theme-color">` tag exists
anywhere in `frontend/app/layout.tsx` (the only `<meta>`-adjacent surface
is the `metadata` export at lines 16–20, which sets only `title` and
`description`) — confirmed by reading the full file and by
`grep -rn "color-scheme\|theme-color\|colorScheme"` across `frontend/`,
which turned up only Mantine's own `data-mantine-color-scheme` attribute
handling (`app/globals.css:19-25`, `app/globals.test.ts`) and
`ThemeToggle.tsx`'s use of Mantine's `useMantineColorScheme`/
`useComputedColorScheme` hooks — no CSS `color-scheme` property override
and no `theme-color`/`color-scheme` meta tag anywhere in this codebase.

## Finding 3 — Dark Reader's actual, current, documented behavior is narrower and messier than "respects `color-scheme`"

Checked against Dark Reader's own GitHub repo (`darkreader/darkreader`),
not secondhand summaries:

- **`CONTRIBUTING.md`** documents one explicit, shipped opt-out: the
  `darkreader-lock` meta tag. "Website pages can request Dark Reader to
  disable itself by embedding a 'Dark Reader lock'. The 'lock' is a
  `<meta>` tag with `name` attribute set to `darkreader-lock`," addable
  statically in `<head>` or dynamically via JS
  (https://github.com/darkreader/darkreader/blob/main/CONTRIBUTING.md).
  This is a full kill-switch for the whole origin, not a
  "we support both schemes" signal — and it's framed for sites that are
  "dark by default, regardless of the system's preferred color scheme," a
  fully-dark site, not a toggleable one like this app.

- **The most current, authoritative statement found** is a maintainer
  (`alexanderby`) reply dated **2026-03-20** in
  https://github.com/darkreader/darkreader/discussions/15128:
  > "The best way to tell Dark Reader about a dark theme is to have
  > `<meta name="color-scheme" content="dark">`. If not present, as soon
  > as there is some style sheet, Dark Reader will check for text and
  > background colors at several points of the screen, and some other
  > heuristics."

  This is recent (six months before this research) and specific: it names
  the **meta tag**, with **`content="dark"` specifically** — not `light
  dark`, not the CSS property form, and it doesn't say whether `light
  dark`/`dark light` (the multi-value "I support both" form this app
  would actually want) is honored the same way.

- **That gap matters, because the multi-value/CSS-property form has a
  documented history of not working reliably**:
  - https://github.com/darkreader/darkreader/discussions/9802 (2022-09-11):
    a developer reported `<meta name="color-scheme" content="dark light">`
    did **not** stop Dark Reader from injecting its own styles; the
    thread's actual resolution was switching to `darkreader-lock`.
  - https://github.com/darkreader/darkreader/issues/13682: Dark Reader
    activating on pages with `:root { color-scheme: light dark
    !important; }` set, reproducible with Dark Reader as the only
    installed extension — filed as a bug, i.e. `light dark` via the CSS
    property is not treated as a reliable opt-out even where the site
    sets it directly and forcefully.
  - https://github.com/darkreader/darkreader/issues/9356, "Dark Reader
    incorrectly handles color-scheme CSS property" — closed via a fix
    (PR #9362), but its existence confirms this has been a genuinely buggy
    area of Dark Reader's own code, not a clean, long-stable feature.
  - https://github.com/darkreader/darkreader/discussions/9016 and
    https://github.com/darkreader/darkreader/issues/15033 are both still
    open feature requests/discussion threads arguing Dark Reader *should*
    treat `color-scheme: dark`/`light dark` as a general "back off"
    signal via `getComputedStyle(document.documentElement)['color-scheme']`
    — i.e. as of this research, that's a proposal being argued for, not
    confirmed-shipped, general behavior.

Put together: Dark Reader has one **clearly confirmed, current** opt-out
signal — `<meta name="color-scheme" content="dark">` — and it is
specifically the single-value form, not the "supports both" form this
app's actual light/dark/auto toggle would need to describe honestly. The
`light dark` / CSS-property route this app already emits via Mantine
(Finding 2) is exactly the form with the rockiest track record in Dark
Reader's own tracker.

## Recommendation

There is a real, narrow, evidence-grounded step available, with an honest
caveat, not a clean silver bullet:

- **What's confirmed to work**: a `<meta name="color-scheme" content="dark">`
  tag, present when the page is *actually* rendering dark. This app's
  resolved scheme is only known at runtime — `ThemeToggle.tsx:36`
  (`useComputedColorScheme('light')`) already computes exactly this value
  client-side, and `app/layout.tsx:106`'s `<ColorSchemeScript
  defaultColorScheme="auto" />` already sets the initial
  `data-mantine-color-scheme` server-side before hydration. A tag whose
  `content` is kept in sync with that resolved value (`"dark"` when
  computed-dark, `"light"` when computed-light) would match what Dark
  Reader's own maintainer describes as reliable — but a **static** tag
  hardcoded to `content="dark"` would be actively wrong half the time
  (this app is light by default/`auto`-light, not always-dark), so this
  is not a one-line addition to `layout.tsx`'s existing static `metadata`
  export — it needs to track the same resolved-scheme state
  `ThemeToggle.tsx` already computes, which is scope for a design/
  implementation pass, not this research doc.
- **What's already present and evidently insufficient**: the CSS
  `color-scheme: light dark` this app already emits via Mantine's base
  stylesheet (Finding 2) is precisely the form Dark Reader's own bug
  tracker shows has an unreliable history (`discussions/9802`,
  `issues/13682`). Don't expect it to start working without the
  meta-tag-based, resolved-value approach above — the evidence doesn't
  support the generic "just add `color-scheme: light dark`" gesture as a
  fix on its own.
- **What fully works but isn't the right shape for this app**: the
  `darkreader-lock` meta tag is a confirmed, documented kill-switch — but
  it disables Dark Reader on the origin outright, and Dark Reader's own
  contributor guidance frames it for sites that are dark by default
  regardless of system preference. This app can be light, dark, or
  system-following depending on the visitor's own choice, so a permanent
  lock would take away a legitimate user choice (someone who prefers Dark
  Reader's rendering even over this app's own dark theme) rather than
  just preventing double-theming — worth ruling out explicitly rather
  than recommending by default.

No further implementation was done here per the research-only scope of
this task.

## Sources

- https://developer.mozilla.org/en-US/docs/Web/CSS/color-scheme
- https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/meta/name/color-scheme
- https://mantine.dev/styles/global-styles/
- https://github.com/mantinedev/mantine/issues/7569
- https://github.com/darkreader/darkreader/blob/main/CONTRIBUTING.md
- https://github.com/darkreader/darkreader/discussions/15128 (maintainer reply dated 2026-03-20)
- https://github.com/darkreader/darkreader/discussions/9802 (2022-09-11)
- https://github.com/darkreader/darkreader/issues/13682
- https://github.com/darkreader/darkreader/issues/9356
- https://github.com/darkreader/darkreader/discussions/9016
- https://github.com/darkreader/darkreader/issues/15033
