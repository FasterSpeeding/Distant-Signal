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

/// A comma-separated list of case-insensitive globs. `*` matches any run of
/// characters (including none); every other character matches itself. No
/// `?`, character classes or path separators: delivery names are plain
/// file names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePattern {
    globs: Vec<String>,
}

impl FilePattern {
    /// Parses `raw`. Errors on an empty list, so a blank env var cannot
    /// silently match nothing (or, for the CIF pattern, stop ingest).
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
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
    pub fn matches(&self, name: &str) -> bool {
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

/// The patterns together, so every caller applies the same precedence.
#[derive(Debug, Clone)]
pub struct Routing {
    pub cif: FilePattern,
    pub cif_exclude: FilePattern,
    /// The CORPUS extract's pattern, or `None` while CORPUS ingest is off
    /// (so a CORPUS file stays a plain stray, as before).
    pub corpus: Option<FilePattern>,
}

impl Routing {
    /// A CIF candidate: matches `cif`, and neither `cif_exclude` nor the
    /// CORPUS pattern (should an operator point that at a zip name).
    pub fn is_cif(&self, name: &str) -> bool {
        self.cif.matches(name) && !self.cif_exclude.matches(name) && !self.is_corpus(name)
    }

    /// A CORPUS candidate (always `false` while CORPUS ingest is off).
    pub fn is_corpus(&self, name: &str) -> bool {
        self.corpus.as_ref().is_some_and(|p| p.matches(name))
    }

    /// The defaults the service ships with (`config.rs`), CORPUS off.
    #[cfg(test)]
    pub fn defaults() -> Self {
        Self {
            cif: FilePattern::parse(crate::config::DEFAULT_CIF_FILE_PATTERN).unwrap(),
            cif_exclude: FilePattern::parse(crate::config::DEFAULT_CIF_EXCLUDE_PATTERN).unwrap(),
            corpus: None,
        }
    }

    /// The defaults with CORPUS ingest on.
    #[cfg(test)]
    pub fn with_corpus() -> Self {
        Self {
            corpus: Some(FilePattern::parse(crate::config::DEFAULT_CORPUS_FILE_PATTERN).unwrap()),
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
            corpus: Some(FilePattern::parse("corpus.zip").unwrap()),
            ..Routing::defaults()
        };
        assert!(!zip_corpus.is_cif("corpus.zip"));
        let routing = Routing::with_corpus();
        assert!(routing.is_cif("timetable_full.zip"));
        assert!(routing.is_cif("TIMETABLE_FULL.ZIP"));
        assert!(!routing.is_cif("readme.txt"));
    }
}
