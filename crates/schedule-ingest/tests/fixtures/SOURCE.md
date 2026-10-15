# Fixture sources

## `nlc_corpus_synthetic.xml`

**Synthetic.** Every code and name in it is invented: the TIPLOCs (`QZ…`),
CRS codes (`QZ…`), NLCs, STANOXes, UIC (`NLCEuroLocation`) codes and
descriptions do not belong to real locations, and were checked to be absent
from the real file. No part of a real delivery is committed.

It is shaped after Network Rail's NLC XML CORPUS format (the newer CORPUS API
version, delivered as dated full snapshots named like `NLC20261009.xml.gz`),
as measured on the 2026-10-09 delivery: the same root, namespaces, item
element, field elements and field order, a single space for a blank value,
`&amp;` in descriptions, a description with trailing padding, and rows with
no `NLCCrsCode` or no `NLCStanoxCode` element at all. See
`crates/schedule-ingest/src/corpus_xml.rs` for the format notes.

## `corpus_extract_excerpt.json`

A hand-trimmed excerpt of a real `CORPUSExtract.json` (the older JSON
format, 2026-09 delivery); see `crates/schedule-ingest/src/corpus.rs`.

## `cif_delivery_excerpt/`

An excerpt of a CIF SCHEDULE delivery; see `crates/schedule-ingest/src/cif_check.rs`.
