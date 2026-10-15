//! The NLC XML CORPUS format: Network Rail's newer CORPUS API version,
//! delivered as dated full snapshots such as `NLC20261009.xml.gz` (gzipped
//! XML, about 7 MB inflated and 350 KB compressed). The older format, the
//! `CORPUSExtract.json.gz` JSON extract, is parsed in `corpus.rs`; both map
//! onto the same [`CorpusLocation`].
//!
//! The shape, measured on the 2026-10-09 delivery (12,102 rows):
//!
//! ```text
//! <ns2:TSDBData xmlns:ns2="http://tempuri.org/XMLSchema.xsd" ...>
//!   <ns2:TSDBDataItem>
//!     <ns2:NLCTiplocCode>…</ns2:NLCTiplocCode>      -> tiploc
//!     <ns2:NLCCapts-Ident>…</ns2:NLCCapts-Ident>    (ignored)
//!     <ns2:NLCLocationCode>…</ns2:NLCLocationCode>  -> nlc (6 digits)
//!     <ns2:NLCCheckChar>…</ns2:NLCCheckChar>        (ignored)
//!     <ns2:NLCDescription>…</ns2:NLCDescription>    -> nlc_desc
//!     <ns2:NLCCapriDesc>…</ns2:NLCCapriDesc>        -> nlc_desc16 (<= 16 chars)
//!     <ns2:NLCCrsCode>…</ns2:NLCCrsCode>            -> crs (optional element)
//!     <ns2:NLCTiplocOwner>…</ns2:NLCTiplocOwner>    (ignored)
//!     <ns2:NLCEuroLocation>…</ns2:NLCEuroLocation>  -> uic
//!     <ns2:NLCStanoxCode>…</ns2:NLCStanoxCode>      -> stanox (optional element)
//!     <ns2:NLCPoMcpCode>…</ns2:NLCPoMcpCode>        (ignored)
//!   </ns2:TSDBDataItem>
//! ```
//!
//! As in the JSON extract, a blank value is a single space; a value is
//! trimmed and a blank becomes `None`. `NLCCrsCode` and `NLCStanoxCode` may
//! also be absent altogether. Elements are matched by local name, so the
//! namespace prefix may change. Unlike the JSON extract, every row carries a
//! TIPLOC: the format holds only the TIPLOC-bearing locations (12,102 rows
//! against the JSON extract's ~56,000, of which ~12,400 have a TIPLOC).
//!
//! Parsing streams: the gzip stream is inflated through a byte cap (the
//! gzip-bomb guard, as in the transit SIRI-SX parser) straight into
//! quick-xml, so the inflated XML is never held whole. Anything that is not
//! this shape -- another root, an unknown element beside the items, a
//! nested element in a field, a field twice in one item, text between
//! items, an unknown entity, a truncated document -- rejects the file. An
//! unknown field inside an item is ignored (a new column is not a reason to
//! stop loading) and named in the result so the caller can log it.

use std::collections::BTreeSet;
use std::io::{BufRead, Read};

use quick_xml::events::Event;

use crate::corpus::{CorpusLocation, Rejected, pad_digits};

/// The root element's local name.
const ROOT: &str = "TSDBData";
/// Each row's local name.
const ITEM: &str = "TSDBDataItem";

/// The item fields this format carries and what each becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Tiploc,
    Nlc,
    Description,
    CapriDesc,
    Crs,
    EuroLocation,
    Stanox,
    /// Known, carried by every row, and not part of the model.
    Ignored,
}

impl Field {
    fn of(local_name: &str) -> Option<Self> {
        Some(match local_name {
            "NLCTiplocCode" => Self::Tiploc,
            "NLCLocationCode" => Self::Nlc,
            "NLCDescription" => Self::Description,
            "NLCCapriDesc" => Self::CapriDesc,
            "NLCCrsCode" => Self::Crs,
            "NLCEuroLocation" => Self::EuroLocation,
            "NLCStanoxCode" => Self::Stanox,
            "NLCCapts-Ident" | "NLCCheckChar" | "NLCTiplocOwner" | "NLCPoMcpCode" => Self::Ignored,
            _ => return None,
        })
    }
}

/// One parsed file.
#[derive(Debug)]
pub(crate) struct NlcXml {
    pub locations: Vec<CorpusLocation>,
    /// Item fields this parser does not know, by local name (ignored).
    pub unknown_fields: BTreeSet<String>,
}

/// The error a [`CappedReader`] raises, told apart from a corrupt gzip
/// stream by downcasting.
#[derive(Debug)]
struct OverCap;

impl std::fmt::Display for OverCap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("inflated past the decompressed-size cap")
    }
}

impl std::error::Error for OverCap {}

/// Reads at most `left` bytes from `inner`, then fails with [`OverCap`]
/// if there is more.
struct CappedReader<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for CappedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(std::io::Error::other(OverCap)),
            };
        }
        let want = usize::try_from(self.left)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let n = self.inner.read(&mut buf[..want])?;
        self.left -= n as u64;
        Ok(n)
    }
}

/// Inflates `compressed` (capped at `max_bytes`) and parses it.
pub(crate) fn parse_gzipped(
    compressed: &[u8],
    max_bytes: u64,
    min_rows: usize,
) -> Result<NlcXml, Rejected> {
    if !compressed.starts_with(&[0x1f, 0x8b]) {
        return Err(Rejected("not a gzip file".to_string()));
    }
    let inflated = CappedReader {
        inner: flate2::read::MultiGzDecoder::new(compressed),
        left: max_bytes,
    };
    parse(std::io::BufReader::new(inflated), min_rows).map_err(|err| match err {
        ParseError::OverCap => Rejected(format!(
            "decompresses to more than CORPUS_MAX_DECOMPRESSED_BYTES ({max_bytes})"
        )),
        ParseError::Io(err) => Rejected(format!("gzip stream is corrupt: {err}")),
        ParseError::Rejected(rejected) => rejected,
    })
}

enum ParseError {
    OverCap,
    Io(String),
    Rejected(Rejected),
}

impl From<Rejected> for ParseError {
    fn from(rejected: Rejected) -> Self {
        Self::Rejected(rejected)
    }
}

fn not_nlc_xml(what: impl std::fmt::Display) -> ParseError {
    ParseError::Rejected(Rejected(format!("not an NLC CORPUS XML file: {what}")))
}

fn xml_error(err: &quick_xml::Error) -> ParseError {
    if let quick_xml::Error::Io(io) = err {
        if io
            .get_ref()
            .is_some_and(|inner| inner.downcast_ref::<OverCap>().is_some())
        {
            return ParseError::OverCap;
        }
        return ParseError::Io(io.to_string());
    }
    not_nlc_xml(err)
}

/// One item being read: a value per mapped field (`None` while unseen or
/// blank), and which fields were seen.
#[derive(Default)]
struct Item {
    tiploc: Option<String>,
    nlc: Option<String>,
    description: Option<String>,
    capri_desc: Option<String>,
    crs: Option<String>,
    euro_location: Option<String>,
    stanox: Option<String>,
    seen: Vec<Field>,
}

impl Item {
    /// Stores one field's trimmed value (`None` when blank), refusing a
    /// field seen twice.
    fn set(&mut self, field: Field, name: &str, text: &str, row: usize) -> Result<(), ParseError> {
        let slot = match field {
            Field::Tiploc => &mut self.tiploc,
            Field::Nlc => &mut self.nlc,
            Field::Description => &mut self.description,
            Field::CapriDesc => &mut self.capri_desc,
            Field::Crs => &mut self.crs,
            Field::EuroLocation => &mut self.euro_location,
            Field::Stanox => &mut self.stanox,
            Field::Ignored => return Ok(()),
        };
        if self.seen.contains(&field) {
            return Err(not_nlc_xml(format_args!("row {row}: {name} appears twice")));
        }
        self.seen.push(field);
        let value = text.trim();
        *slot = (!value.is_empty()).then(|| value.to_string());
        Ok(())
    }

    fn finish(self, row: usize) -> Result<CorpusLocation, ParseError> {
        let nlc = self
            .nlc
            .ok_or_else(|| Rejected(format!("row {row} has no NLC")))?;
        Ok(CorpusLocation {
            nlc: pad_digits(nlc, 6),
            stanox: self.stanox.map(|s| pad_digits(s, 5)),
            tiploc: self.tiploc,
            crs: self.crs,
            uic: self.euro_location,
            nlc_desc: self.description,
            nlc_desc16: self.capri_desc,
        })
    }
}

/// Where the reader is in the document.
enum At {
    /// Before the root element.
    Prolog,
    /// Inside the root, between items.
    Root,
    /// Inside an item, between fields.
    Item(Item),
    /// Inside one field of an item (`None` for an unknown field).
    Field(Item, Option<Field>, String),
    /// After the root element closed.
    Epilog,
}

/// Appends one entity reference (`&amp;`, `&#38;`) to `text`; an entity XML
/// does not predefine refuses the file.
fn push_ref(
    text: &mut String,
    reference: &quick_xml::events::BytesRef<'_>,
) -> Result<(), ParseError> {
    if let Ok(Some(ch)) = reference.resolve_char_ref() {
        text.push(ch);
        return Ok(());
    }
    let name = reference.decode().map_err(not_nlc_xml)?;
    text.push_str(match name.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        other => return Err(not_nlc_xml(format_args!("unknown entity &{other};"))),
    });
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "one event loop over the document's states; splitting it would scatter the state machine"
)]
fn parse<R: BufRead>(input: R, min_rows: usize) -> Result<NlcXml, ParseError> {
    let mut reader = quick_xml::Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut at = At::Prolog;
    let mut locations = Vec::new();
    let mut unknown_fields = BTreeSet::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|err| xml_error(&err))?;
        at = match (at, event) {
            (at, Event::Comment(_) | Event::PI(_)) => at,
            (At::Prolog, Event::Decl(_) | Event::DocType(_)) => At::Prolog,
            (at @ (At::Prolog | At::Root | At::Item(_) | At::Epilog), Event::Text(text)) => {
                let text = text.decode().map_err(not_nlc_xml)?;
                if !text.trim().is_empty() {
                    return Err(not_nlc_xml("text outside a field"));
                }
                at
            }
            (At::Prolog, Event::Start(e)) if e.local_name().as_ref() == ROOT.as_bytes() => At::Root,
            // An empty root has no rows: the row minimum rejects it.
            (At::Prolog, Event::Empty(e)) if e.local_name().as_ref() == ROOT.as_bytes() => {
                At::Epilog
            }
            (At::Prolog, Event::Start(e) | Event::Empty(e)) => {
                return Err(not_nlc_xml(format_args!(
                    "the root element is <{}>, not <{ROOT}>",
                    String::from_utf8_lossy(e.local_name().as_ref())
                )));
            }
            (At::Root, Event::Start(e)) if e.local_name().as_ref() == ITEM.as_bytes() => {
                At::Item(Item::default())
            }
            (At::Root, Event::End(_)) => At::Epilog,
            (At::Item(item), Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                let field = Field::of(&name);
                if field.is_none() {
                    unknown_fields.insert(name.clone());
                }
                At::Field(item, field, String::new())
            }
            (At::Item(mut item), Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match Field::of(&name) {
                    Some(field) => item.set(field, &name, "", locations.len())?,
                    None => {
                        unknown_fields.insert(name);
                    }
                }
                At::Item(item)
            }
            (At::Item(item), Event::End(_)) => {
                locations.push(item.finish(locations.len())?);
                At::Root
            }
            (At::Field(item, field, mut text), Event::Text(t)) => {
                text.push_str(&t.decode().map_err(not_nlc_xml)?);
                At::Field(item, field, text)
            }
            (At::Field(item, field, mut text), Event::CData(t)) => {
                text.push_str(&t.decode().map_err(not_nlc_xml)?);
                At::Field(item, field, text)
            }
            (At::Field(item, field, mut text), Event::GeneralRef(r)) => {
                push_ref(&mut text, &r)?;
                At::Field(item, field, text)
            }
            (At::Field(mut item, field, text), Event::End(e)) => {
                if let Some(field) = field {
                    let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                    item.set(field, &name, &text, locations.len())?;
                }
                At::Item(item)
            }
            (At::Field(..), Event::Start(e) | Event::Empty(e)) => {
                return Err(not_nlc_xml(format_args!(
                    "row {}: unexpected <{}> inside a field",
                    locations.len(),
                    String::from_utf8_lossy(e.local_name().as_ref())
                )));
            }
            (At::Epilog, Event::Eof) => break,
            (_, Event::Eof) => return Err(not_nlc_xml("the document ends early (truncated?)")),
            (_, Event::Start(e) | Event::Empty(e)) => {
                return Err(not_nlc_xml(format_args!(
                    "unexpected element <{}> after row {}",
                    String::from_utf8_lossy(e.local_name().as_ref()),
                    locations.len()
                )));
            }
            (_, other) => {
                return Err(not_nlc_xml(format_args!(
                    "unexpected {other:?} after row {}",
                    locations.len()
                )));
            }
        };
        buf.clear();
    }
    if locations.len() < min_rows {
        return Err(ParseError::Rejected(Rejected(format!(
            "only {} rows, fewer than CORPUS_XML_MIN_ROWS ({min_rows})",
            locations.len()
        ))));
    }
    Ok(NlcXml {
        locations,
        unknown_fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::tests::gzip;

    /// A small synthetic file in the real shape (invented codes and names;
    /// see `tests/fixtures/SOURCE.md`).
    pub(crate) const FIXTURE: &str = include_str!("../tests/fixtures/nlc_corpus_synthetic.xml");

    fn parse_str(xml: &str, min_rows: usize) -> Result<NlcXml, Rejected> {
        parse_gzipped(&gzip(xml.as_bytes()), 1024 * 1024, min_rows)
    }

    fn doc(items: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<ns2:TSDBData xmlns:ns2="http://tempuri.org/XMLSchema.xsd">{items}</ns2:TSDBData>"#
        )
    }

    #[test]
    fn the_real_shape_parses_and_normalises() {
        let parsed = parse_str(FIXTURE, 1).unwrap();
        let rows = parsed.locations;
        assert_eq!(rows.len(), 6);
        assert!(
            parsed.unknown_fields.is_empty(),
            "{:?}",
            parsed.unknown_fields
        );
        assert_eq!(
            rows[0],
            CorpusLocation {
                nlc: "370100".to_string(),
                stanox: Some("90101".to_string()),
                tiploc: Some("QZAPLTN".to_string()),
                crs: Some("QZA".to_string()),
                uic: Some("37010".to_string()),
                nlc_desc: Some("APPLETON QUAY".to_string()),
                nlc_desc16: None,
            }
        );
        // `&amp;` unescaped; NLCCapriDesc kept as nlc_desc16.
        assert_eq!(rows[1].nlc_desc.as_deref(), Some("BRAMBLE & THORN"));
        assert_eq!(rows[1].nlc_desc16.as_deref(), Some("BRAMBLE&THORN"));
        // No NLCCrsCode element at all; a blank NLCEuroLocation.
        assert_eq!(rows[2].crs, None);
        assert_eq!(rows[2].uic, None);
        assert_eq!(rows[2].stanox.as_deref(), Some("90103"));
        // No NLCStanoxCode element; a blank NLCCrsCode; a padded
        // description trimmed.
        assert_eq!(rows[3].stanox, None);
        assert_eq!(rows[3].crs, None);
        assert_eq!(rows[3].nlc_desc.as_deref(), Some("CRESTWICK SIG QX12"));
        // Two TIPLOCs sharing one STANOX, as real ones do.
        assert_eq!(rows[4].stanox, rows[5].stanox);
    }

    #[test]
    fn empty_elements_and_unknown_fields_are_tolerated() {
        let parsed = parse_str(
            &doc(
                "<ns2:TSDBDataItem><ns2:NLCLocationCode>1200</ns2:NLCLocationCode>\
                 <ns2:NLCCrsCode/><ns2:NLCNewThing>x</ns2:NLCNewThing>\
                 <ns2:NLCStanoxCode>4311</ns2:NLCStanoxCode></ns2:TSDBDataItem>",
            ),
            1,
        )
        .unwrap();
        assert_eq!(parsed.locations[0].nlc, "001200");
        assert_eq!(parsed.locations[0].stanox.as_deref(), Some("04311"));
        assert_eq!(parsed.locations[0].crs, None);
        assert_eq!(
            parsed.unknown_fields.into_iter().collect::<Vec<_>>(),
            ["NLCNewThing"]
        );
    }

    #[test]
    fn anything_else_is_rejected() {
        let item = "<ns2:TSDBDataItem><ns2:NLCLocationCode>990100</ns2:NLCLocationCode></ns2:TSDBDataItem>";
        let cases = [
            (
                r#"{"TIPLOCDATA":[]}"#.to_string(),
                "not an NLC CORPUS XML file",
            ),
            ("<Other/>".to_string(), "root element is <Other>"),
            (
                doc("<ns2:Header>x</ns2:Header>"),
                "unexpected element <Header>",
            ),
            (doc("stray"), "text outside a field"),
            (
                doc(
                    "<ns2:TSDBDataItem><ns2:NLCTiplocCode>A</ns2:NLCTiplocCode></ns2:TSDBDataItem>",
                ),
                "row 0 has no NLC",
            ),
            (
                doc(
                    "<ns2:TSDBDataItem><ns2:NLCLocationCode> </ns2:NLCLocationCode></ns2:TSDBDataItem>",
                ),
                "row 0 has no NLC",
            ),
            (
                doc(&format!(
                    "{item}<ns2:TSDBDataItem><ns2:NLCLocationCode>1</ns2:NLCLocationCode>\
                     <ns2:NLCLocationCode>2</ns2:NLCLocationCode></ns2:TSDBDataItem>"
                )),
                "row 1: NLCLocationCode appears twice",
            ),
            (
                doc(
                    "<ns2:TSDBDataItem><ns2:NLCLocationCode>1<b/></ns2:NLCLocationCode></ns2:TSDBDataItem>",
                ),
                "unexpected <b> inside a field",
            ),
            (
                doc(
                    "<ns2:TSDBDataItem><ns2:NLCLocationCode>&nbsp;1</ns2:NLCLocationCode></ns2:TSDBDataItem>",
                ),
                "unknown entity &nbsp;",
            ),
            (doc(item).replace("</ns2:TSDBData>", ""), "ends early"),
        ];
        for (xml, expected) in cases {
            let err = parse_str(&xml, 1).unwrap_err();
            assert!(err.0.contains(expected), "{xml}: {err}");
        }
        let too_few = parse_str(FIXTURE, 7).unwrap_err();
        assert!(too_few.0.contains("CORPUS_XML_MIN_ROWS"), "{too_few}");
    }

    #[test]
    fn gzip_is_required_and_the_inflated_size_is_capped() {
        let err = parse_gzipped(FIXTURE.as_bytes(), 1024 * 1024, 1).unwrap_err();
        assert_eq!(err.0, "not a gzip file");
        let err = parse_gzipped(&gzip(FIXTURE.as_bytes()), 100, 1).unwrap_err();
        assert!(
            err.0.contains("CORPUS_MAX_DECOMPRESSED_BYTES (100)"),
            "{err}"
        );
        let exact = u64::try_from(FIXTURE.len()).unwrap();
        assert!(parse_gzipped(&gzip(FIXTURE.as_bytes()), exact, 1).is_ok());
        let mut corrupt = gzip(FIXTURE.as_bytes());
        let mid = corrupt.len() / 2;
        corrupt[mid] ^= 0xff;
        corrupt.truncate(mid + 4);
        let err = parse_gzipped(&corrupt, 1024 * 1024, 1).unwrap_err();
        assert!(
            err.0.contains("gzip stream is corrupt") || err.0.contains("not an NLC"),
            "{err}"
        );
    }

    /// Checks a full, real file (never committed) when pointed at one:
    /// `CORPUS_XML_SAMPLE=/path/NLC20261009.xml.gz cargo test -p
    /// schedule-ingest -- --ignored full_real_nlc_xml`.
    #[test]
    #[ignore = "needs CORPUS_XML_SAMPLE pointing at a real NLC*.xml.gz"]
    fn full_real_nlc_xml_parses_under_the_defaults() {
        let Ok(path) = std::env::var("CORPUS_XML_SAMPLE") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let parsed = parse_gzipped(&bytes, 256 * 1024 * 1024, 10_000).unwrap();
        let rows = &parsed.locations;
        println!(
            "rows={} tiploc={} stanox={} crs={} uic={} desc16={} unknown_fields={:?}",
            rows.len(),
            rows.iter().filter(|r| r.tiploc.is_some()).count(),
            rows.iter().filter(|r| r.stanox.is_some()).count(),
            rows.iter().filter(|r| r.crs.is_some()).count(),
            rows.iter().filter(|r| r.uic.is_some()).count(),
            rows.iter().filter(|r| r.nlc_desc16.is_some()).count(),
            parsed.unknown_fields,
        );
        assert!(rows.len() > 10_000, "{}", rows.len());
        assert!(parsed.unknown_fields.is_empty());
        assert!(rows.iter().all(|r| r.nlc.len() == 6 && r.tiploc.is_some()));
        assert!(
            rows.iter()
                .filter_map(|r| r.crs.as_deref())
                .all(|crs| crs.len() == 3)
        );
        assert!(
            rows.iter()
                .any(|r| r.nlc_desc.as_deref().is_some_and(|d| d.contains('&')))
        );
    }
}
