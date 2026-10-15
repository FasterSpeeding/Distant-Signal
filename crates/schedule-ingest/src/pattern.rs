//! File-name patterns that route a file landing in `watch_dir` to the right
//! pipeline.
//!
//! The SFTP account now receives more than the CIF SCHEDULE zip: Network
//! Rail's CORPUS extract (`CORPUSExtract.json.gz`) and a SMART berth file
//! that is also named `CORPUSExtract` (`CORPUSExtract.csv.gz`). CIF
//! selection used to take the newest `*.zip`, so either pushed as a zip
//! could have been extracted and published as the day's timetable. A file
//! is now a CIF candidate only if it matches `cif` and not `cif_exclude`
//! (`CORPUSExtract*` by default, whatever the extension). While CORPUS
//! ingest is on, a file matching `corpus` (`CORPUSExtract.json.gz`) goes to
//! `corpus.rs` instead. Anything else is left in place and gets the
//! existing one-time "stray file" warning.
//!
//! Since 2026-10 Network Rail also delivers CORPUS as dated NLC XML
//! snapshots (`NLC20261009.xml.gz`, the newer CORPUS API version). These
//! are picked by their `NLC` prefix and `.xml.gz` extension
//! ([`is_nlc_xml_name`]), never by a configurable glob, and any other
//! `NLC*` file is format drift ([`is_nlc_drift_name`]).

use crate::corpus::CorpusFormat;

/// A comma-separated list of case-insensitive globs. `*` matches any run of
/// characters (including none); every other character matches itself. No
/// `?`, character classes or path separators: delivery names are plain
/// file names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePattern {
    globs: Vec<String>,
}

impl FilePattern {
    /// Parses `raw`. Errors on an empty list, so a blank env var cannot
    /// silently match nothing (or, for the CIF pattern, stop ingest).
    pub(crate) fn parse(raw: &str) -> anyhow::Result<Self> {
        let globs: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|glob| !glob.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        if globs.is_empty() {
            anyhow::bail!("file pattern {raw:?} lists no globs");
        }
        if let Some(glob) = globs.iter().find(|glob| glob.contains('/')) {
            anyhow::bail!("file pattern glob {glob:?} must be a plain file name, not a path");
        }
        Ok(Self { globs })
    }

    /// Whether `name` matches any glob in the list.
    pub(crate) fn matches(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.globs.iter().any(|glob| glob_matches(glob, &name))
    }
}

/// Classic greedy wildcard match with backtracking to the last `*`, over
/// bytes (both sides are already lowercased; patterns are ASCII in
/// practice, and a non-ASCII byte only ever matches itself or a `*`).
fn glob_matches(glob: &str, name: &str) -> bool {
    let (glob, name) = (glob.as_bytes(), name.as_bytes());
    let (mut g, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if g < glob.len() && glob[g] == b'*' {
            star = Some((g, n));
            g += 1;
        } else if g < glob.len() && glob[g] == name[n] {
            g += 1;
            n += 1;
        } else if let Some((star_g, star_n)) = star {
            g = star_g + 1;
            n = star_n + 1;
            star = Some((star_g, star_n + 1));
        } else {
            return false;
        }
    }
    glob[g..].iter().all(|&b| b == b'*')
}

/// The NLC XML CORPUS snapshot's name: this prefix and [`NLC_XML_SUFFIX`]
/// (both case-insensitive), e.g. `NLC20261009.xml.gz`.
pub(crate) const NLC_PREFIX: &str = "nlc";
/// See [`NLC_PREFIX`].
pub(crate) const NLC_XML_SUFFIX: &str = ".xml.gz";

/// Whether `name` carries the NLC CORPUS prefix, whatever its extension.
pub(crate) fn has_nlc_prefix(name: &str) -> bool {
    name.get(..NLC_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(NLC_PREFIX))
}

/// Whether `name` is an NLC XML CORPUS snapshot (`NLC*.xml.gz`).
pub(crate) fn is_nlc_xml_name(name: &str) -> bool {
    has_nlc_prefix(name)
        && name.len() >= NLC_PREFIX.len() + NLC_XML_SUFFIX.len()
        && name
            .get(name.len() - NLC_XML_SUFFIX.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(NLC_XML_SUFFIX))
}

/// Whether `name` looks like the NLC CORPUS feed but is not `.xml.gz`:
/// the format may have drifted (`corpus.rs` reports it; never loaded).
pub(crate) fn is_nlc_drift_name(name: &str) -> bool {
    has_nlc_prefix(name) && !is_nlc_xml_name(name)
}

/// Which CORPUS formats are loaded (`None` in [`Routing`] while CORPUS
/// ingest is off).
#[derive(Debug, Clone)]
pub(crate) struct CorpusRouting {
    /// The JSON extract's pattern, or `None` while that format is off.
    pub json: Option<FilePattern>,
    /// Whether the NLC XML snapshot (`NLC*.xml.gz`) is loaded.
    pub xml: bool,
}

/// The patterns together, so every caller applies the same precedence.
#[derive(Debug, Clone)]
pub(crate) struct Routing {
    pub cif: FilePattern,
    pub cif_exclude: FilePattern,
    /// The CORPUS formats, or `None` while CORPUS ingest is off (so a
    /// CORPUS file stays a plain stray, as before).
    pub corpus: Option<CorpusRouting>,
}

impl Routing {
    /// A CIF candidate: matches `cif`, and neither `cif_exclude`, a CORPUS
    /// format (should an operator point that at a zip name) nor the NLC
    /// CORPUS prefix.
    pub(crate) fn is_cif(&self, name: &str) -> bool {
        self.cif.matches(name)
            && !self.cif_exclude.matches(name)
            && !self.is_corpus(name)
            && !has_nlc_prefix(name)
    }

    /// A CORPUS candidate (always `false` while CORPUS ingest is off).
    pub(crate) fn is_corpus(&self, name: &str) -> bool {
        self.corpus_format(name).is_some()
    }

    /// The CORPUS format `name` is loaded as, if any. An `NLC*.xml.gz`
    /// name is the XML snapshot even if the JSON pattern matches it too.
    pub(crate) fn corpus_format(&self, name: &str) -> Option<CorpusFormat> {
        let corpus = self.corpus.as_ref()?;
        if is_nlc_xml_name(name) {
            return corpus.xml.then_some(CorpusFormat::Xml);
        }
        corpus
            .json
            .as_ref()
            .is_some_and(|p| p.matches(name))
            .then_some(CorpusFormat::Json)
    }

    /// The defaults the service ships with (`config.rs`), CORPUS off.
    #[cfg(test)]
    pub(crate) fn defaults() -> Self {
        Self {
            cif: FilePattern::parse(crate::config::DEFAULT_CIF_FILE_PATTERN).unwrap(),
            cif_exclude: FilePattern::parse(crate::config::DEFAULT_CIF_EXCLUDE_PATTERN).unwrap(),
            corpus: None,
        }
    }

    /// The defaults with CORPUS ingest on (both formats).
    #[cfg(test)]
    pub(crate) fn with_corpus() -> Self {
        Self {
            corpus: Some(CorpusRouting {
                json: Some(FilePattern::parse(crate::config::DEFAULT_CORPUS_FILE_PATTERN).unwrap()),
                xml: true,
            }),
            ..Self::defaults()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_case_insensitively_with_star_wildcards() {
        let pattern = FilePattern::parse("CORPUSExtract*").unwrap();
        assert!(pattern.matches("CORPUSExtract.json.gz"));
        assert!(pattern.matches("corpusextract.csv"));
        assert!(pattern.matches("CORPUSExtract"));
        assert!(!pattern.matches("xCORPUSExtract.json.gz"));

        let pattern = FilePattern::parse("*.zip, a*b*c").unwrap();
        assert!(pattern.matches("timetable_full.ZIP"));
        assert!(pattern.matches("abc"));
        assert!(pattern.matches("aXXbYYc"));
        assert!(pattern.matches("abcbc"));
        assert!(!pattern.matches("abcd"));
        assert!(!pattern.matches("zip"));
    }

    #[test]
    fn empty_or_path_shaped_patterns_are_rejected() {
        assert!(FilePattern::parse("").is_err());
        assert!(FilePattern::parse(" , ").is_err());
        assert!(FilePattern::parse("incoming/*.zip").is_err());
    }

    /// The CIF guard: nothing named `CORPUSExtract*` -- the CORPUS JSON,
    /// the SMART `.csv.gz` sharing its name, or a zip of either -- is ever
    /// a CIF candidate, while the real CIF delivery name still is.
    #[test]
    fn corpus_named_files_in_every_format_are_never_cif_candidates() {
        for routing in [Routing::defaults(), Routing::with_corpus()] {
            for name in [
                "CORPUSExtract.json.gz",
                "CORPUSExtract.csv.gz",
                "CORPUSExtract.json",
                "CORPUSExtract.csv",
                "CORPUSExtract.zip",
                "CORPUSExtract.json.zip",
                "CORPUSExtract.csv.zip",
                "corpusextract.ZIP",
            ] {
                assert!(!routing.is_cif(name), "{name}");
            }
        }
        // Only the JSON extract is a CORPUS candidate, and only when on;
        // the SMART `.csv.gz` never is.
        let on = Routing::with_corpus();
        assert!(on.is_corpus("CORPUSExtract.json.gz"));
        assert!(on.is_corpus("corpusextract.JSON.GZ"));
        assert!(!on.is_corpus("CORPUSExtract.csv.gz"));
        assert!(!on.is_corpus("CORPUSExtract.zip"));
        assert!(!Routing::defaults().is_corpus("CORPUSExtract.json.gz"));

        // A CORPUS pattern pointed at a zip name still keeps it out of CIF.
        let zip_corpus = Routing {
            cif_exclude: FilePattern::parse("nothing-matches-this").unwrap(),
            corpus: Some(CorpusRouting {
                json: Some(FilePattern::parse("corpus.zip").unwrap()),
                xml: false,
            }),
            ..Routing::defaults()
        };
        assert!(!zip_corpus.is_cif("corpus.zip"));
        let routing = Routing::with_corpus();
        assert!(routing.is_cif("timetable_full.zip"));
        assert!(routing.is_cif("TIMETABLE_FULL.ZIP"));
        assert!(!routing.is_cif("readme.txt"));
    }

    /// The NLC XML snapshot is picked by its `NLC` prefix and `.xml.gz`
    /// extension only; any other `NLC*` name is drift, never a candidate
    /// of either pipeline.
    #[test]
    fn nlc_xml_names_and_drift_names() {
        for name in ["NLC20261009.xml.gz", "nlc20261009.XML.GZ", "NLC.xml.gz"] {
            assert!(is_nlc_xml_name(name), "{name}");
            assert!(!is_nlc_drift_name(name), "{name}");
        }
        for name in [
            "NLC20261009.xml",
            "NLC20261009.zip",
            "NLC20261009.json.gz",
            "NLC20261009.xml.gz.part",
            "nlc",
        ] {
            assert!(!is_nlc_xml_name(name), "{name}");
            assert!(is_nlc_drift_name(name), "{name}");
        }
        for name in [
            "CORPUSExtract.json.gz",
            "XNLC20261009.xml.gz",
            "NL.xml.gz",
            "",
        ] {
            assert!(!is_nlc_xml_name(name) && !is_nlc_drift_name(name), "{name}");
        }

        let on = Routing::with_corpus();
        assert_eq!(
            on.corpus_format("NLC20261009.xml.gz"),
            Some(CorpusFormat::Xml)
        );
        assert_eq!(
            on.corpus_format("CORPUSExtract.json.gz"),
            Some(CorpusFormat::Json)
        );
        assert_eq!(on.corpus_format("NLC20261009.zip"), None);
        assert!(!on.is_cif("NLC20261009.zip"));
        // Each format can be switched off on its own.
        let json_only = Routing {
            corpus: Some(CorpusRouting {
                json: on.corpus.as_ref().unwrap().json.clone(),
                xml: false,
            }),
            ..Routing::defaults()
        };
        assert_eq!(json_only.corpus_format("NLC20261009.xml.gz"), None);
        assert_eq!(
            json_only.corpus_format("CORPUSExtract.json.gz"),
            Some(CorpusFormat::Json)
        );
        let xml_only = Routing {
            corpus: Some(CorpusRouting {
                json: None,
                xml: true,
            }),
            ..Routing::defaults()
        };
        assert_eq!(xml_only.corpus_format("CORPUSExtract.json.gz"), None);
        assert_eq!(
            xml_only.corpus_format("NLC20261009.xml.gz"),
            Some(CorpusFormat::Xml)
        );
        // A JSON pattern that also matches an NLC snapshot does not make it
        // JSON.
        let greedy = Routing {
            corpus: Some(CorpusRouting {
                json: Some(FilePattern::parse("*.gz").unwrap()),
                xml: false,
            }),
            ..Routing::defaults()
        };
        assert_eq!(greedy.corpus_format("NLC20261009.xml.gz"), None);
    }
}
