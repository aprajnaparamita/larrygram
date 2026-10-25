//! `cargo xtask scout`: fetch and classify the wide corpus (task P8-16).
//!
//! `cargo xtask scout fetch <csv>` reads the BigQuery export of public
//! GitHub files (`repo_name,ref,path,blob_id,license`), downloads every
//! distinct blob once into `reference/wide-corpus/<tier>/<blob_id>.<ext>`,
//! verifies its git blob SHA-1 against the export, and pins each kept
//! file to a commit with `git ls-remote`. The run is resumable: the
//! append-only `fetch.csv` state lets a stopped run continue where it
//! stopped.
//!
//! `cargo xtask scout classify` turns the fetched files into `index.csv`
//! (one row per kept file: identity, real kind, the `.vsd` version byte,
//! stencil/template reality and the Microsoft-content filter) and
//! `excluded.csv` (the filtered-out files). The OLE facts of `.vsd`,
//! `.vss` and `.vst` files come from `ole-facts.csv`, which the sweep
//! tests write through `larrygram-vsd-binary`'s public API; xtask reads
//! the OLE side of a file only through that file.
//!
//! Everything lands under `reference/wide-corpus/` (gitignored, ADR
//! 0005): no file-specific content is ever committed. Files excluded by
//! the Microsoft-content filter (AGENTS.md section 4) are never read by
//! a test, a sweep or an export.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sha1::{Digest as _, Sha1};
use sha2::Sha256;

use crate::util::Result;

/// The default output directory (inside the gitignored `reference/`).
pub const OUT_DIR: &str = "reference/wide-corpus";
/// The fetch state file, appended as each blob completes.
pub const FETCH_CSV: &str = "fetch.csv";
/// The `git ls-remote` cache, appended as each repo is resolved.
pub const PINS_TSV: &str = "pins.tsv";
/// The classification facts the sweep tests write for OLE files.
pub const OLE_FACTS_CSV: &str = "ole-facts.csv";
/// The per-file classification index.
pub const INDEX_CSV: &str = "index.csv";
/// The excluded files, with their reason.
pub const EXCLUDED_CSV: &str = "excluded.csv";
/// The size cap of one download (20 MB).
pub const MAX_DOWNLOAD: u64 = 20 * 1024 * 1024;
/// How many requests run at once (a few: polite to the host).
pub const WORKERS: usize = 4;
/// Retries per download before the row is recorded as failed.
pub const RETRIES: usize = 2;
/// The base back-off of a retry, in milliseconds.
pub const RETRY_BASE_MS: u64 = 250;

/// What `cargo xtask scout` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoutCommand {
    /// `scout fetch <csv>`: download and pin the blobs of the export.
    Fetch {
        /// The BigQuery export to read.
        csv: PathBuf,
    },
    /// `scout classify`: write `index.csv` and `excluded.csv`.
    Classify,
}

/// Parsed command-line options for the scout task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The subcommand.
    pub command: ScoutCommand,
    /// The output directory (default `reference/wide-corpus`).
    pub out: Option<PathBuf>,
}

/// Parses the arguments after `cargo xtask scout`.
pub fn parse_args(args: &[String]) -> Result<Options> {
    let mut out = None;
    let mut positional: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => {
                let dir = it.next().ok_or("--out needs a directory")?;
                out = Some(PathBuf::from(dir));
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown scout argument `{other}`"));
            }
            other => positional.push(other.to_owned()),
        }
    }
    let Some(verb) = positional.first().map(String::as_str) else {
        return Err("scout needs a subcommand: fetch <csv> or classify".into());
    };
    let command = match verb {
        "fetch" => {
            let Some(csv) = positional.get(1) else {
                return Err("scout fetch needs the BigQuery export path".into());
            };
            if positional.len() > 2 {
                return Err(format!(
                    "scout fetch takes one csv (got {})",
                    positional.len() - 1
                ));
            }
            ScoutCommand::Fetch {
                csv: PathBuf::from(csv),
            }
        }
        "classify" => {
            if positional.len() > 1 {
                return Err("scout classify takes no arguments".into());
            }
            ScoutCommand::Classify
        }
        other => return Err(format!("unknown scout subcommand `{other}`")),
    };
    Ok(Options { command, out })
}

// ---------------------------------------------------------------- CSV

/// Parses RFC 4180 CSV into rows of fields.
///
/// Handles quoted fields (with `""` escapes), embedded commas and
/// embedded newlines, and accepts both `\n` and `\r\n` line endings.
/// Never panics on hostile input.
pub fn csv_rows(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => in_quotes = true,
            '"' => return Err("unexpected quote inside an unquoted CSV field".into()),
            ',' => row.push(std::mem::take(&mut field)),
            '\n' | '\r' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if in_quotes {
        return Err("unterminated quoted CSV field".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows
        .into_iter()
        .filter(|r| !(r.len() == 1 && r.first().is_some_and(String::is_empty)))
        .collect())
}

/// Quotes one CSV field when it needs it.
pub fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Formats one CSV row (without the trailing newline).
pub fn csv_line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",")
}

// ------------------------------------------------------------ the export

/// One row of the BigQuery export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRow {
    /// The GitHub `owner/name` of the repository.
    pub repo: String,
    /// The git ref, e.g. `refs/heads/master`.
    pub git_ref: String,
    /// The path of the file inside the repository.
    pub path: String,
    /// The git blob SHA-1 of the file (40 lowercase hex digits).
    pub blob_id: String,
    /// The SPDX license id the dataset reports for the repository.
    pub license: String,
}

/// Parses the BigQuery export (`repo_name,ref,path,blob_id,license`).
pub fn repo_rows(text: &str) -> Result<Vec<RepoRow>> {
    let rows = csv_rows(text)?;
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i == 0 {
            let header: Vec<&str> = row.iter().map(String::as_str).collect();
            if header != ["repo_name", "ref", "path", "blob_id", "license"] {
                return Err(format!(
                    "expected header `repo_name,ref,path,blob_id,license`, got {}",
                    csv_line(row)
                ));
            }
            continue;
        }
        if row.len() != 5 {
            return Err(format!(
                "export row {i}: expected 5 fields, got {}",
                row.len()
            ));
        }
        let blob_id = row
            .get(3)
            .map(String::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if blob_id.len() != 40 || !blob_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("export row {i}: `{}` is not a blob id", row[3]));
        }
        out.push(RepoRow {
            repo: row[0].clone(),
            git_ref: row[1].clone(),
            path: row[2].clone(),
            blob_id,
            license: row[4].clone(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- tiers

/// The licence tier of a file (task P8-16 goal 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Permissive: may enter the manifest and the goldens.
    A,
    /// GPL family: local sweeps only, never committed, never named.
    B,
}

impl Tier {
    /// The directory name under the output directory.
    pub fn dir(self) -> &'static str {
        match self {
            Tier::A => "a",
            Tier::B => "b",
        }
    }
}

/// The tier of a licence id, or `None` for a licence outside both tiers
/// (such files are never fetched).
pub fn tier_of(license: &str) -> Option<Tier> {
    let lower = license.to_ascii_lowercase();
    match lower.as_str() {
        "mit" | "apache-2.0" | "bsd-2-clause" | "bsd-3-clause" | "isc" | "mpl-2.0" | "epl-1.0"
        | "cc0-1.0" | "unlicense" | "artistic-2.0" => Some(Tier::A),
        "gpl-2.0" | "gpl-3.0" | "lgpl-2.1" | "lgpl-3.0" | "agpl-3.0" => Some(Tier::B),
        _ => None,
    }
}

// ------------------------------------------------------------ extensions

/// The Visio file extensions the export covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileExt {
    /// Binary drawing.
    Vsd,
    /// OPC drawing.
    Vsdx,
    /// Binary stencil.
    Vss,
    /// OPC stencil.
    Vssx,
    /// Binary template.
    Vst,
    /// OPC template.
    Vstx,
    /// XML drawing (out of the alpha scope: classify only).
    Vdx,
}

impl FileExt {
    /// The extension without the dot.
    pub fn as_str(self) -> &'static str {
        match self {
            FileExt::Vsd => "vsd",
            FileExt::Vsdx => "vsdx",
            FileExt::Vss => "vss",
            FileExt::Vssx => "vssx",
            FileExt::Vst => "vst",
            FileExt::Vstx => "vstx",
            FileExt::Vdx => "vdx",
        }
    }

    /// Whether the file is an OLE compound file (`.vsd`, `.vss`, `.vst`).
    pub fn is_ole(self) -> bool {
        matches!(self, FileExt::Vsd | FileExt::Vss | FileExt::Vst)
    }
}

/// The extension of a path, when it is one this task covers.
pub fn ext_of(path: &str) -> Option<FileExt> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "vsd" => Some(FileExt::Vsd),
        "vsdx" => Some(FileExt::Vsdx),
        "vss" => Some(FileExt::Vss),
        "vssx" => Some(FileExt::Vssx),
        "vst" => Some(FileExt::Vst),
        "vstx" => Some(FileExt::Vstx),
        "vdx" => Some(FileExt::Vdx),
        _ => None,
    }
}

// ---------------------------------------------------------------- plan

/// One distinct blob to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The GitHub `owner/name` the blob is fetched from.
    pub repo: String,
    /// The git ref of that repository.
    pub git_ref: String,
    /// The path of the file inside the repository.
    pub path: String,
    /// The git blob SHA-1 the download must reproduce.
    pub blob_id: String,
    /// The SPDX licence id.
    pub license: String,
    /// The licence tier.
    pub tier: Tier,
    /// The file extension.
    pub ext: FileExt,
}

/// The fetch plan: one entry per distinct blob, plus the drop counts.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// The blobs to fetch, in first-seen order.
    pub files: Vec<Planned>,
    /// Export rows seen in total.
    pub rows: usize,
    /// Rows dropped because the blob was already planned.
    pub duplicates: usize,
    /// Rows dropped for a licence outside both tiers.
    pub no_tier: usize,
    /// Rows dropped for an extension outside the Visio set.
    pub no_ext: usize,
}

/// Deduplicates the export rows into a fetch plan: the first occurrence
/// of a blob names the file that is downloaded.
pub fn plan(rows: &[RepoRow]) -> Plan {
    let mut out = Plan {
        rows: rows.len(),
        ..Plan::default()
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        if !seen.insert(row.blob_id.clone()) {
            out.duplicates += 1;
            continue;
        }
        let Some(tier) = tier_of(&row.license) else {
            out.no_tier += 1;
            continue;
        };
        let Some(ext) = ext_of(&row.path) else {
            out.no_ext += 1;
            continue;
        };
        out.files.push(Planned {
            repo: row.repo.clone(),
            git_ref: row.git_ref.clone(),
            path: row.path.clone(),
            blob_id: row.blob_id.clone(),
            license: row.license.clone(),
            tier,
            ext,
        });
    }
    out
}

// -------------------------------------------------------------- digests

/// The git blob SHA-1 of `data` (`sha1("blob <len>\0" + data)`).
pub fn git_blob_sha1(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", data.len()).as_bytes());
    hasher.update(data);
    hex(&hasher.finalize())
}

/// The lowercase hex spelling of a digest.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The lowercase hex sha256 of `data` (the corpus checker's own).
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex(&digest)
}

/// Percent-encodes one path segment, keeping `/` separators.
pub fn encode_path(path: &str) -> String {
    let mut out = String::new();
    for byte in path.bytes() {
        let keep = byte == b'/'
            || byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The `raw.githubusercontent.com` URL of one export row.
pub fn raw_url(repo: &str, git_ref: &str, path: &str) -> String {
    let branch = git_ref
        .strip_prefix("refs/heads/")
        .or_else(|| git_ref.strip_prefix("refs/tags/"))
        .unwrap_or(git_ref);
    format!(
        "https://raw.githubusercontent.com/{repo}/{}/{}",
        encode_path(branch),
        encode_path(path)
    )
}

/// Parses one `git ls-remote` line list into the commit of the first
/// matching ref.
pub fn parse_ls_remote(text: &str) -> Result<String> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(sha), Some(_ref)) = (parts.next(), parts.next()) else {
            continue;
        };
        if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(sha.to_ascii_lowercase());
        }
    }
    Err("git ls-remote: ref not found".into())
}

// ------------------------------------------------------------- fetch state

/// One row of `fetch.csv`, the append-only fetch state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRow {
    /// The git blob SHA-1 of the file.
    pub blob_id: String,
    /// The tier directory (`a` or `b`).
    pub tier: String,
    /// The SPDX licence id.
    pub license: String,
    /// The GitHub `owner/name` the file came from.
    pub repo: String,
    /// The pinned commit, empty when the pin failed.
    pub commit: String,
    /// The upstream path.
    pub path: String,
    /// The file extension.
    pub ext: String,
    /// The sha256 of the downloaded bytes.
    pub sha256: String,
    /// The size in bytes.
    pub size: u64,
    /// `kept`, `mismatch`, `too-big` or `failed`.
    pub status: String,
    /// A human-readable note (why a row is not `kept`).
    pub note: String,
}

impl StateRow {
    /// The row as one `fetch.csv` line.
    pub fn line(&self) -> String {
        csv_line(&[
            self.blob_id.clone(),
            self.tier.clone(),
            self.license.clone(),
            self.repo.clone(),
            self.commit.clone(),
            self.path.clone(),
            self.ext.clone(),
            self.sha256.clone(),
            self.size.to_string(),
            self.status.clone(),
            self.note.clone(),
        ])
    }
}

/// The header line of `fetch.csv`.
pub fn state_header() -> String {
    "blob_id,tier,license,repo,commit,path,ext,sha256,size,status,note".to_owned()
}

/// Parses `fetch.csv` (header plus rows; older or partial files parse
/// as far as they go).
pub fn state_rows(text: &str) -> Result<Vec<StateRow>> {
    let rows = csv_rows(text)?;
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i == 0 && row.len() == 11 && row.first().map(String::as_str) == Some("blob_id") {
            continue;
        }
        if row.len() != 11 {
            return Err(format!(
                "fetch.csv row {}: expected 11 fields, got {}",
                i,
                row.len()
            ));
        }
        let get = |j: usize| -> String { row.get(j).cloned().unwrap_or_default() };
        let size = get(8).parse::<u64>().unwrap_or(0);
        out.push(StateRow {
            blob_id: get(0),
            tier: get(1),
            license: get(2),
            repo: get(3),
            commit: get(4),
            path: get(5),
            ext: get(6),
            sha256: get(7),
            size,
            status: get(9),
            note: get(10),
        });
    }
    Ok(out)
}

/// The last state row per blob (the file is append-only).
pub fn state_by_blob(rows: &[StateRow]) -> BTreeMap<String, StateRow> {
    let mut out = BTreeMap::new();
    for row in rows {
        out.insert(row.blob_id.clone(), row.clone());
    }
    out
}

/// Reads `pins.tsv` (`repo<TAB>ref<TAB>commit` per line).
pub fn read_pins(text: &str) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(repo), Some(git_ref), Some(commit)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        out.insert((repo.to_owned(), git_ref.to_owned()), commit.to_owned());
    }
    out
}

// ---------------------------------------------------------------- network

/// Why a download did not produce bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The body (or its `Content-Length`) exceeds the size cap.
    TooBig(u64),
    /// The request failed for another reason.
    Failed(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::TooBig(size) => write!(f, "file is {size} bytes, over the size cap"),
            NetError::Failed(why) => write!(f, "{why}"),
        }
    }
}

/// The two network calls of the fetch, injectable so the tests run
/// without a network.
pub trait ScoutNet: Sync {
    /// GETs `url` and returns the body.
    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, NetError>;
    /// Resolves `git_ref` of `repo` (`owner/name`) to a commit.
    fn pin(&self, repo: &str, git_ref: &str) -> Result<String>;
}

/// The production [`ScoutNet`]: raw.githubusercontent.com over ureq,
/// and `git ls-remote` for the pins.
pub struct LiveNet {
    agent: ureq::Agent,
}

impl LiveNet {
    /// Builds the agent (rustls with the platform root store, the same
    /// configuration as [`crate::corpus`]).
    pub fn new() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(platform_roots())
            .build();
        let agent = ureq::Agent::config_builder().tls_config(tls).build().into();
        Self { agent }
    }
}

impl Default for LiveNet {
    fn default() -> Self {
        Self::new()
    }
}

/// Loads the OS trust store and converts it into ureq's `RootCerts`.
fn platform_roots() -> ureq::tls::RootCerts {
    let loaded = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = loaded
        .certs
        .iter()
        .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
        .collect();
    certs.into()
}

impl ScoutNet for LiveNet {
    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, NetError> {
        let mut response = self
            .agent
            .get(url)
            .call()
            .map_err(|e| NetError::Failed(format!("GET {url}: {e}")))?;
        if response.status() != 200 {
            return Err(NetError::Failed(format!(
                "GET {url}: HTTP {}",
                response.status()
            )));
        }
        if let Some(len) = response.body_mut().content_length()
            && len > MAX_DOWNLOAD
        {
            return Err(NetError::TooBig(len));
        }
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_DOWNLOAD + 1)
            .read_to_end(&mut body)
            .map_err(|e| NetError::Failed(format!("GET {url}: read body: {e}")))?;
        if body.len() as u64 > MAX_DOWNLOAD {
            return Err(NetError::TooBig(body.len() as u64));
        }
        Ok(body)
    }

    fn pin(&self, repo: &str, git_ref: &str) -> Result<String> {
        let url = format!("https://github.com/{repo}");
        let output = Command::new("git")
            .args(["ls-remote", &url, git_ref])
            .output()
            .map_err(|e| format!("git ls-remote {url}: {e}"))?;
        if !output.status.success() {
            return Err(format!("git ls-remote {url}: exit {}", output.status));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        parse_ls_remote(&text)
    }
}

// ------------------------------------------------------------- fetch run

/// The counts of one fetch run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchSummary {
    /// Files downloaded and verified in this run.
    pub kept: usize,
    /// Files already on disk with the right blob id.
    pub present: usize,
    /// Files dropped: the blob id did not verify (the branch moved).
    pub mismatch: usize,
    /// Files dropped: over the size cap.
    pub too_big: usize,
    /// Files not fetched after the retries.
    pub failed: usize,
    /// Kept files whose commit could not be pinned.
    pub unpinned: usize,
}

/// The file names of one blob in the output directory.
pub fn file_path(out_dir: &Path, planned: &Planned) -> PathBuf {
    out_dir
        .join(planned.tier.dir())
        .join(format!("{}.{}", planned.blob_id, planned.ext.as_str()))
}

/// Whether `path` holds the bytes of `blob_id` right now.
fn file_matches(path: &Path, blob_id: &str) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => git_blob_sha1(&bytes) == blob_id,
        Err(_) => false,
    }
}

/// One download job: the row, the URL and the destination file.
struct Job {
    planned: Planned,
    url: String,
    dest: PathBuf,
}

/// What a worker made of one job.
struct Outcome {
    planned: Planned,
    commit: String,
    status: String,
    note: String,
    sha256: String,
    size: u64,
}

/// Runs the fetch over the plan: downloads, verifies, writes, pins and
/// appends one `fetch.csv` row per attempted blob. Resumable: blobs
/// with a `kept` row and a matching file are skipped, permanent drops
/// are never retried.
pub fn run_fetch(
    net: &dyn ScoutNet,
    out_dir: &Path,
    plan: &Plan,
    workers: usize,
    retry_base_ms: u64,
) -> Result<FetchSummary> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut summary = FetchSummary::default();

    let state_path = out_dir.join(FETCH_CSV);
    let state = read_state_file(&state_path)?;
    let pins_path = out_dir.join(PINS_TSV);
    let pins = match std::fs::read_to_string(&pins_path) {
        Ok(text) => read_pins(&text),
        Err(_) => BTreeMap::new(),
    };

    let mut jobs = Vec::new();
    for planned in &plan.files {
        let dest = file_path(out_dir, planned);
        match state.get(&planned.blob_id) {
            Some(row) if row.status == "kept" && file_matches(&dest, &planned.blob_id) => {
                summary.present += 1;
                continue;
            }
            Some(row) if row.status == "mismatch" || row.status == "too-big" => continue,
            _ => {}
        }
        jobs.push(Job {
            planned: planned.clone(),
            url: raw_url(&planned.repo, &planned.git_ref, &planned.path),
            dest,
        });
    }

    let state_file = open_append(&state_path, &state_header())?;
    let pin_file = open_append(&pins_path, "")?;
    let state_file = Mutex::new(state_file);
    let pin_file = Mutex::new(pin_file);
    let pins = Mutex::new(pins);
    let counts = Mutex::new(FetchSummary::default());
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let workers = workers.max(1);
    let total = jobs.len();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(job) = jobs.get(i) else {
                        break;
                    };
                    let outcome = run_job(net, job, retry_base_ms, &pins, &pin_file);
                    let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
                    if finished % 250 == 0 {
                        eprintln!("scout: {finished}/{total} attempted");
                    }
                    {
                        let mut counts = lock(&counts);
                        match outcome.status.as_str() {
                            "kept" => {
                                counts.kept += 1;
                                if outcome.commit.is_empty() {
                                    counts.unpinned += 1;
                                }
                            }
                            "mismatch" => counts.mismatch += 1,
                            "too-big" => counts.too_big += 1,
                            _ => counts.failed += 1,
                        }
                    }
                    let row = StateRow {
                        blob_id: outcome.planned.blob_id.clone(),
                        tier: outcome.planned.tier.dir().to_owned(),
                        license: outcome.planned.license.clone(),
                        repo: outcome.planned.repo.clone(),
                        commit: outcome.commit,
                        path: outcome.planned.path.clone(),
                        ext: outcome.planned.ext.as_str().to_owned(),
                        sha256: outcome.sha256,
                        size: outcome.size,
                        status: outcome.status,
                        note: outcome.note,
                    };
                    if let Err(e) = append_line(&state_file, &row.line()) {
                        eprintln!("scout: {e}");
                    }
                }
            });
        }
    });

    let counts = lock(&counts);
    summary.kept = counts.kept;
    summary.mismatch = counts.mismatch;
    summary.too_big = counts.too_big;
    summary.failed = counts.failed;
    summary.unpinned = counts.unpinned;
    Ok(summary)
}

/// Fetches, verifies, writes and pins one blob.
fn run_job(
    net: &dyn ScoutNet,
    job: &Job,
    retry_base_ms: u64,
    pins: &Mutex<BTreeMap<(String, String), String>>,
    pin_file: &Mutex<File>,
) -> Outcome {
    let planned = &job.planned;
    let mut note = String::new();
    let mut bytes: Option<Vec<u8>> = None;
    for attempt in 0..=RETRIES {
        match net.get(&job.url) {
            Ok(body) => {
                bytes = Some(body);
                break;
            }
            Err(NetError::TooBig(size)) => {
                return Outcome {
                    planned: planned.clone(),
                    commit: String::new(),
                    status: "too-big".to_owned(),
                    note: format!("{size} bytes, over the {} byte cap", MAX_DOWNLOAD),
                    sha256: String::new(),
                    size: 0,
                };
            }
            Err(NetError::Failed(why)) => {
                note = why;
                if attempt < RETRIES {
                    std::thread::sleep(Duration::from_millis(
                        retry_base_ms.saturating_mul(u64::try_from(attempt).unwrap_or(0) + 1),
                    ));
                }
            }
        }
    }
    let Some(bytes) = bytes else {
        return Outcome {
            planned: planned.clone(),
            commit: String::new(),
            status: "failed".to_owned(),
            note,
            sha256: String::new(),
            size: 0,
        };
    };
    let size = bytes.len() as u64;
    let sha256 = sha256_hex(&bytes);
    if git_blob_sha1(&bytes) != planned.blob_id {
        return Outcome {
            planned: planned.clone(),
            commit: String::new(),
            status: "mismatch".to_owned(),
            note: "the blob id does not verify: the branch moved on".to_owned(),
            sha256,
            size,
        };
    }
    if let Err(e) = crate::util::write_atomic_bytes(&job.dest, &bytes) {
        return Outcome {
            planned: planned.clone(),
            commit: String::new(),
            status: "failed".to_owned(),
            note: format!("cannot write {}: {e}", job.dest.display()),
            sha256,
            size,
        };
    }
    let key = (planned.repo.clone(), planned.git_ref.clone());
    let cached = {
        let cache = lock(pins);
        cache.get(&key).cloned()
    };
    let commit = match cached {
        Some(commit) => commit,
        None => match net.pin(&planned.repo, &planned.git_ref) {
            Ok(commit) => {
                let line = format!("{}\t{}\t{}\n", planned.repo, planned.git_ref, commit);
                if let Err(e) = append_line(pin_file, &line) {
                    eprintln!("scout: {e}");
                }
                lock(pins).insert(key, commit.clone());
                commit
            }
            Err(e) => {
                note = format!("unpinned: {e}");
                String::new()
            }
        },
    };
    Outcome {
        planned: planned.clone(),
        commit,
        status: "kept".to_owned(),
        note,
        sha256,
        size,
    }
}

/// Locks a mutex, taking the value out of a poisoned lock instead of
/// panicking.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens `path` for appending, writing `header` when the file is new.
fn open_append(path: &Path, header: &str) -> Result<File> {
    let new = !path.exists();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    if new && !header.is_empty() {
        file.write_all(format!("{header}\n").as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(file)
}

/// Appends one line to an open state file.
fn append_line(file: &Mutex<File>, line: &str) -> Result<()> {
    lock(file)
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("cannot append to the state file: {e}"))
}

/// Reads and parses `fetch.csv`, tolerating a missing file and a
/// partially written last line (a run that was killed mid-append).
fn read_state_file(path: &Path) -> Result<BTreeMap<String, StateRow>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Ok(BTreeMap::new()),
    };
    match state_rows(&text) {
        Ok(rows) => Ok(state_by_blob(&rows)),
        Err(_) => {
            let mut text = text;
            loop {
                let Some(cut) = text.rfind('\n') else {
                    return Ok(BTreeMap::new());
                };
                text.truncate(cut);
                if let Ok(rows) = state_rows(&text) {
                    return Ok(state_by_blob(&rows));
                }
            }
        }
    }
}

// ------------------------------------------------------------ classification

/// What the classifier learned about one file (goal 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The real kind, e.g. `ole-stencil`, `ooxml-drawing`, `vdx`.
    pub kind: String,
    /// The `.vsd` version byte, when known.
    pub version: String,
    /// Whether the kind matches the file extension (`yes`, `no`,
    /// `unknown`).
    pub kind_matches_ext: String,
    /// A document property names Microsoft as the author.
    pub ms_author: bool,
    /// A document property names Microsoft as the company.
    pub ms_company: bool,
    /// The file is a copy of a Visio stock template or stencil.
    pub template_copy: bool,
    /// The file was made from a Visio stock template (its masters may
    /// be copies of Microsoft masters).
    pub template_origin: bool,
}

impl Facts {
    /// The exclusion reason, or `None` when the file is in scope
    /// (goal 4).
    pub fn excluded_reason(&self) -> Option<&'static str> {
        if self.ms_author {
            Some("microsoft-author")
        } else if self.ms_company {
            Some("microsoft-company")
        } else if self.template_copy {
            Some("template-copy")
        } else if self.template_origin {
            Some("template-origin")
        } else {
            None
        }
    }
}

/// Whether a property value names Microsoft (case-insensitive).
pub fn contains_microsoft(value: &str) -> bool {
    value.to_ascii_lowercase().contains("microsoft")
}

/// Whether `name` is a Visio stock template or stencil identifier
/// (`BASICD_M.VSTX` and friends).
pub fn is_stock_template_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .rsplit_once('.')
        .map(|(stem, _ext)| stem)
        .unwrap_or(lower.as_str());
    const KNOWN: [&str; 3] = ["basicd_m", "basflo_m", "xfunc_m"];
    if KNOWN.contains(&stem) {
        return true;
    }
    // `<CODE>_M` with a short alphanumeric code: the naming of the
    // templates and stencils that ship with Visio.
    let Some((code, suffix)) = stem.rsplit_once('_') else {
        return false;
    };
    suffix == "m"
        && (5..=8).contains(&code.len())
        && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The facts of the file `bytes`, from its own bytes alone (no OLE
/// reading: those facts arrive through `ole-facts.csv`).
pub fn classify_bytes(bytes: &[u8], ext: FileExt, file_name: &str) -> Facts {
    if bytes.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
        return zip_facts(bytes, ext, file_name);
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        // An OLE compound file: the sweep tests name the stream, the
        // version and the properties.
        return Facts {
            kind: "ole".to_owned(),
            kind_matches_ext: "unknown".to_owned(),
            ..Facts::default()
        };
    }
    if ext == FileExt::Vdx && looks_like_xml(bytes) {
        return Facts {
            kind: "vdx".to_owned(),
            kind_matches_ext: "yes".to_owned(),
            ..Facts::default()
        };
    }
    Facts {
        kind: "other".to_owned(),
        kind_matches_ext: "no".to_owned(),
        ..Facts::default()
    }
}

/// Whether the bytes start with XML (after a BOM and whitespace).
fn looks_like_xml(bytes: &[u8]) -> bool {
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(body);
    text.trim_start().starts_with('<')
}

/// The facts of an OPC package (`.vsdx`, `.vssx`, `.vstx`): the main
/// content type, and the `docProps` properties the filter reads.
fn zip_facts(bytes: &[u8], ext: FileExt, file_name: &str) -> Facts {
    let mut archive = match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
        Ok(archive) => archive,
        Err(_) => {
            return Facts {
                kind: "zip-invalid".to_owned(),
                ..Facts::default()
            };
        }
    };
    let names: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok())
        .map(|n| n.into_owned())
        .collect();
    let has_document = names.iter().any(|n| n == "visio/document.xml");
    let content_types = read_zip_entry(&mut archive, "[Content_Types].xml", 4 * 1024 * 1024);
    let main_type = content_types
        .as_deref()
        .and_then(|xml| override_type(xml, "/visio/document.xml"));
    let kind = match (has_document, main_type.as_deref()) {
        (true, Some("application/vnd.ms-visio.drawing.main+xml")) => "ooxml-drawing",
        (true, Some("application/vnd.ms-visio.stencil.main+xml")) => "ooxml-stencil",
        (true, Some("application/vnd.ms-visio.template.main+xml")) => "ooxml-template",
        (true, _) => "ooxml-other",
        (false, _) => "zip-other",
    };
    let core = read_zip_entry(&mut archive, "docProps/core.xml", 4 * 1024 * 1024);
    let app = read_zip_entry(&mut archive, "docProps/app.xml", 4 * 1024 * 1024);
    let creator = core.as_deref().and_then(|xml| tag_text(xml, "creator"));
    let title = core.as_deref().and_then(|xml| tag_text(xml, "title"));
    let company = app.as_deref().and_then(|xml| tag_text(xml, "Company"));
    let template = app.as_deref().and_then(|xml| tag_text(xml, "Template"));
    facts_from_properties(
        kind,
        ext,
        file_name,
        creator.as_deref(),
        company.as_deref(),
        title.as_deref(),
        template.as_deref(),
    )
}

/// Builds the filter flags from the document properties (the shared
/// rule of goals 3 and 4).
fn facts_from_properties(
    kind: &str,
    ext: FileExt,
    file_name: &str,
    creator: Option<&str>,
    company: Option<&str>,
    title: Option<&str>,
    template: Option<&str>,
) -> Facts {
    let ms_author = creator.is_some_and(contains_microsoft);
    let ms_company = company.is_some_and(contains_microsoft);
    let template_copy =
        is_stock_template_name(file_name) || title.is_some_and(is_stock_template_name);
    let template_origin = template.is_some_and(is_stock_template_name);
    Facts {
        kind: kind.to_owned(),
        version: String::new(),
        kind_matches_ext: kind_matches_ext(kind, ext),
        ms_author,
        ms_company,
        template_copy,
        template_origin,
    }
}

/// Whether the real kind agrees with the file extension (`yes`, `no`,
/// or `unknown` when the kind does not say).
pub fn kind_matches_ext(kind: &str, ext: FileExt) -> String {
    let matches = match kind {
        "ooxml-drawing" | "ole-drawing" | "vdx" => {
            ext == FileExt::Vsd || ext == FileExt::Vsdx || ext == FileExt::Vdx
        }
        "ooxml-stencil" | "ole-stencil" => ext == FileExt::Vss || ext == FileExt::Vssx,
        "ooxml-template" | "ole-template" => ext == FileExt::Vst || ext == FileExt::Vstx,
        _ => return "unknown".to_owned(),
    };
    if matches {
        "yes".to_owned()
    } else {
        "no".to_owned()
    }
}

/// Reads one zip entry, capped at `cap` bytes.
fn read_zip_entry(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
    cap: u64,
) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    entry
        .take(cap)
        .read_to_end(&mut buf)
        .ok()
        .map(|_| String::from_utf8_lossy(&buf).into_owned())
}

/// The `ContentType` of the `<Override>` for `part` in
/// `[Content_Types].xml`, when the XML names it.
pub fn override_type(xml: &str, part: &str) -> Option<String> {
    for element in xml.split("<Override").skip(1) {
        let Some(end) = element.find('>') else {
            continue;
        };
        let attributes = &element[..end];
        let Some(name) = attribute_value(attributes, "PartName") else {
            continue;
        };
        if name != part {
            continue;
        }
        return attribute_value(attributes, "ContentType");
    }
    None
}

/// The value of one XML attribute in a tag's attribute text.
fn attribute_value(attributes: &str, name: &str) -> Option<String> {
    let mut rest = attributes;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let mut chars = after.chars();
        let quote = chars.next()?;
        if quote != '"' && quote != '\'' {
            rest = &rest[eq + 1..];
            continue;
        }
        let after = &after[1..];
        let end = after.find(quote)?;
        let value = &after[..end];
        let local = key.rsplit(':').next().unwrap_or(key);
        if local == name {
            return Some(value.to_owned());
        }
        rest = &after[end + 1..];
    }
    None
}

/// The text of the first element with this local name (namespace
/// prefixes are ignored), as `docProps` spells its property elements.
pub fn tag_text(xml: &str, local_name: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        let end = after.find('>')?;
        let tag = &after[..end];
        rest = &after[end + 1..];
        if tag.starts_with('/') || tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if tag.ends_with('/') {
            continue;
        }
        let name = tag
            .split([' ', '\t', '\r', '\n', '/'])
            .next()
            .unwrap_or_default();
        let matches = name == local_name
            || name
                .rsplit_once(':')
                .is_some_and(|(_prefix, local)| local == local_name);
        if !matches {
            continue;
        }
        let close = format!("</{name}>");
        let close_at = rest.find(&close)?;
        return Some(rest[..close_at].trim().to_owned());
    }
    None
}

// --------------------------------------------------------------- ole facts

/// One row of `ole-facts.csv`, written by the sweep tests through
/// `larrygram-vsd-binary`'s public API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OleFactsRow {
    /// The git blob SHA-1 (also the file name stem).
    pub blob_id: String,
    /// `ole-drawing`, `ole-stencil`, `ole-template` or `ole-unknown`.
    pub kind: String,
    /// The version byte as decimal, empty when unreadable.
    pub version: String,
    /// A document property names Microsoft as the author.
    pub ms_author: bool,
    /// A document property names Microsoft as the company.
    pub ms_company: bool,
    /// The file is a copy of a Visio stock template or stencil.
    pub template_copy: bool,
    /// The file was made from a Visio stock template.
    pub template_origin: bool,
}

/// Parses `ole-facts.csv`.
pub fn ole_facts_rows(text: &str) -> Result<Vec<OleFactsRow>> {
    let rows = csv_rows(text)?;
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i == 0 && row.len() == 7 && row.first().map(String::as_str) == Some("blob_id") {
            continue;
        }
        if row.len() != 7 {
            return Err(format!(
                "ole-facts.csv row {}: expected 7 fields, got {}",
                i,
                row.len()
            ));
        }
        let get = |j: usize| -> String { row.get(j).cloned().unwrap_or_default() };
        out.push(OleFactsRow {
            blob_id: get(0),
            kind: get(1),
            version: get(2),
            ms_author: get(3) == "1",
            ms_company: get(4) == "1",
            template_copy: get(5) == "1",
            template_origin: get(6) == "1",
        });
    }
    Ok(out)
}

/// One `index.csv` row: the identity of a kept file plus its facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    /// The fetch state row (identity, size, sha256).
    pub state: StateRow,
    /// The classification facts.
    pub facts: Facts,
}

impl IndexRow {
    /// The row as one `index.csv` line.
    pub fn line(&self) -> String {
        let reason = self.facts.excluded_reason().unwrap_or_default();
        csv_line(&[
            self.state.blob_id.clone(),
            self.state.tier.clone(),
            self.state.license.clone(),
            self.state.repo.clone(),
            self.state.commit.clone(),
            self.state.path.clone(),
            self.state.ext.clone(),
            self.state.sha256.clone(),
            self.state.size.to_string(),
            self.facts.kind.clone(),
            self.facts.version.clone(),
            self.facts.kind_matches_ext.clone(),
            flag(self.facts.ms_author),
            flag(self.facts.ms_company),
            flag(self.facts.template_copy),
            flag(self.facts.template_origin),
            flag(self.facts.excluded_reason().is_some()).to_owned(),
            reason.to_owned(),
        ])
    }
}

/// The header line of `index.csv` (and of `excluded.csv`).
pub fn index_header() -> String {
    "blob_id,tier,license,repo,commit,path,ext,sha256,size,kind,version,kind_matches_ext,\
     ms_author,ms_company,template_copy,template_origin,excluded,exclude_reason"
        .to_owned()
}

fn flag(value: bool) -> String {
    if value {
        "1".to_owned()
    } else {
        "0".to_owned()
    }
}

/// The counts of one classify run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassifySummary {
    /// Kept files classified.
    pub files: usize,
    /// Files excluded by the Microsoft-content filter.
    pub excluded: usize,
    /// Kept OLE files whose facts `ole-facts.csv` did not name.
    pub ole_missing: usize,
}

/// Runs the classification: reads every kept file, merges the OLE
/// facts, applies the filter, and writes `index.csv` and
/// `excluded.csv`.
pub fn run_classify(out_dir: &Path) -> Result<ClassifySummary> {
    let state_path = out_dir.join(FETCH_CSV);
    let state = read_state_file(&state_path)?;
    let ole_path = out_dir.join(OLE_FACTS_CSV);
    let mut ole: BTreeMap<String, OleFactsRow> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(&ole_path) {
        for row in ole_facts_rows(&text)? {
            ole.insert(row.blob_id.clone(), row);
        }
    }

    let mut summary = ClassifySummary::default();
    let mut index: Vec<IndexRow> = Vec::new();
    let mut excluded: Vec<IndexRow> = Vec::new();
    for (blob_id, row) in &state {
        if row.status != "kept" {
            continue;
        }
        let path = out_dir
            .join(&row.tier)
            .join(format!("{}.{}", blob_id, row.ext));
        let bytes =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let name = row
            .path
            .rsplit('/')
            .next()
            .unwrap_or(row.path.as_str())
            .to_owned();
        let ext = ext_of(&row.ext).ok_or_else(|| format!("unknown extension {}", row.ext))?;
        let mut facts = classify_bytes(&bytes, ext, &name);
        if ext.is_ole() {
            match ole.get(blob_id) {
                Some(ole_row) => {
                    facts.kind = ole_row.kind.clone();
                    facts.version = ole_row.version.clone();
                    facts.ms_author = ole_row.ms_author;
                    facts.ms_company = ole_row.ms_company;
                    facts.template_copy = ole_row.template_copy;
                    facts.template_origin = ole_row.template_origin;
                    facts.kind_matches_ext = kind_matches_ext(&facts.kind, ext);
                }
                None => summary.ole_missing += 1,
            }
        }
        let entry = IndexRow {
            state: row.clone(),
            facts,
        };
        summary.files += 1;
        if entry.facts.excluded_reason().is_some() {
            summary.excluded += 1;
            excluded.push(entry);
        } else {
            index.push(entry);
        }
    }

    let mut index_text = format!("{}\n", index_header());
    for row in &index {
        index_text.push_str(&row.line());
        index_text.push('\n');
    }
    let mut excluded_text = format!("{}\n", index_header());
    for row in &excluded {
        excluded_text.push_str(&row.line());
        excluded_text.push('\n');
    }
    crate::util::write_atomic_bytes(&out_dir.join(INDEX_CSV), index_text.as_bytes())?;
    crate::util::write_atomic_bytes(&out_dir.join(EXCLUDED_CSV), excluded_text.as_bytes())?;
    Ok(summary)
}

// ----------------------------------------------------------------- driver

/// Runs the scout task with already-parsed options.
pub fn run(opts: &Options) -> Result<()> {
    let out_dir = match &opts.out {
        Some(dir) => dir.clone(),
        None => crate::util::root().join(OUT_DIR),
    };
    match &opts.command {
        ScoutCommand::Fetch { csv } => {
            let text = std::fs::read_to_string(csv)
                .map_err(|e| format!("cannot read {}: {e}", csv.display()))?;
            let rows = repo_rows(&text)?;
            let plan = plan(&rows);
            println!(
                "scout: {} rows, {} distinct blobs to fetch ({} without a tier, {} without a Visio extension)",
                plan.rows,
                plan.files.len(),
                plan.no_tier,
                plan.no_ext
            );
            let net = LiveNet::new();
            let summary = run_fetch(&net, &out_dir, &plan, WORKERS, RETRY_BASE_MS)?;
            println!(
                "scout: kept {}, present {}, mismatch {}, too-big {}, failed {}, unpinned {}",
                summary.kept,
                summary.present,
                summary.mismatch,
                summary.too_big,
                summary.failed,
                summary.unpinned
            );
            Ok(())
        }
        ScoutCommand::Classify => {
            let summary = run_classify(&out_dir)?;
            println!(
                "scout: classified {} files, excluded {} ({} OLE files had no facts)",
                summary.files, summary.excluded, summary.ole_missing
            );
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::TempDir;
    use std::sync::atomic::AtomicUsize;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The header line of `ole-facts.csv` (the same one the sweep writes).
    fn ole_facts_header() -> String {
        "blob_id,kind,version,ms_author,ms_company,template_copy,template_origin".to_owned()
    }

    #[test]
    fn parses_the_command_line() {
        let opts = parse_args(&args(&["fetch", "x.csv"])).unwrap();
        assert_eq!(
            opts.command,
            ScoutCommand::Fetch {
                csv: PathBuf::from("x.csv")
            }
        );
        assert_eq!(opts.out, None);
        let opts = parse_args(&args(&["classify", "--out", "/tmp/w"])).unwrap();
        assert_eq!(opts.command, ScoutCommand::Classify);
        assert_eq!(opts.out, Some(PathBuf::from("/tmp/w")));
    }

    #[test]
    fn rejects_bad_command_lines() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&args(&["fetch"])).is_err());
        assert!(parse_args(&args(&["fetch", "a.csv", "b.csv"])).is_err());
        assert!(parse_args(&args(&["classify", "extra"])).is_err());
        assert!(parse_args(&args(&["bogus"])).is_err());
        assert!(parse_args(&args(&["--out"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
    }

    #[test]
    fn parses_csv_rows() {
        assert_eq!(
            csv_rows("a,b\n1,2\n").unwrap(),
            vec![
                vec!["a".to_owned(), "b".to_owned()],
                vec!["1".to_owned(), "2".to_owned()]
            ]
        );
        // quoted commas and escaped quotes
        assert_eq!(
            csv_rows("\"a,b\",\"say \"\"hi\"\"\"\n").unwrap(),
            vec![vec!["a,b".to_owned(), "say \"hi\"".to_owned()]]
        );
        // CRLF and an embedded newline
        assert_eq!(
            csv_rows("\"a\nb\",c\r\nd,e").unwrap(),
            vec![
                vec!["a\nb".to_owned(), "c".to_owned()],
                vec!["d".to_owned(), "e".to_owned()]
            ]
        );
        // an empty trailing line is dropped
        assert_eq!(csv_rows("a\n\n").unwrap(), vec![vec!["a".to_owned()]]);
        assert!(csv_rows("\"a").is_err());
        assert!(csv_rows("a\"b,c\n").is_err());
    }

    #[test]
    fn quotes_csv_fields() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_line(&["a".to_owned(), "b,c".to_owned()]), "a,\"b,c\"");
    }

    #[test]
    fn parses_the_export() {
        let text = "repo_name,ref,path,blob_id,license\n\
                    o/r,refs/heads/master,\"a b/x.vsd\",AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA,mit\n";
        let rows = repo_rows(text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].repo, "o/r");
        assert_eq!(rows[0].path, "a b/x.vsd");
        assert_eq!(rows[0].blob_id, "a".repeat(40));
        assert_eq!(rows[0].license, "mit");

        assert!(repo_rows("not,the,header\n").is_err());
        assert!(repo_rows("repo_name,ref,path,blob_id,license\na,b,c,short,mit\n").is_err());
        assert!(repo_rows("repo_name,ref,path,blob_id,license\na,b\n").is_err());
    }

    #[test]
    fn tiers_match_the_task_file() {
        for license in [
            "mit",
            "apache-2.0",
            "bsd-2-clause",
            "bsd-3-clause",
            "isc",
            "mpl-2.0",
            "epl-1.0",
            "cc0-1.0",
            "unlicense",
            "artistic-2.0",
            "MIT",
        ] {
            assert_eq!(tier_of(license), Some(Tier::A), "{license}");
        }
        for license in ["gpl-2.0", "gpl-3.0", "lgpl-2.1", "lgpl-3.0", "agpl-3.0"] {
            assert_eq!(tier_of(license), Some(Tier::B), "{license}");
        }
        assert_eq!(tier_of("none"), None);
        assert_eq!(Tier::A.dir(), "a");
        assert_eq!(Tier::B.dir(), "b");
    }

    #[test]
    fn extensions_match_the_export() {
        assert_eq!(ext_of("a/b.vsd"), Some(FileExt::Vsd));
        assert_eq!(ext_of("x.VsDx"), Some(FileExt::Vsdx));
        assert_eq!(ext_of("a.vss"), Some(FileExt::Vss));
        assert_eq!(ext_of("a.vssx"), Some(FileExt::Vssx));
        assert_eq!(ext_of("a.vst"), Some(FileExt::Vst));
        assert_eq!(ext_of("a.vstx"), Some(FileExt::Vstx));
        assert_eq!(ext_of("a.vdx"), Some(FileExt::Vdx));
        assert_eq!(ext_of("a.txt"), None);
        assert_eq!(ext_of("a/b"), None);
        assert_eq!(ext_of("dir.vsd/no-ext"), None);
        assert!(FileExt::Vsd.is_ole());
        assert!(!FileExt::Vdx.is_ole());
    }

    fn row(path: &str, blob: char, license: &str) -> RepoRow {
        RepoRow {
            repo: "o/r".to_owned(),
            git_ref: "refs/heads/master".to_owned(),
            path: path.to_owned(),
            blob_id: blob.to_string().repeat(40),
            license: license.to_owned(),
        }
    }

    #[test]
    fn plans_one_entry_per_blob() {
        let rows = vec![
            row("a.vsd", 'a', "mit"),
            row("copy-of-a.vsd", 'a', "mit"),
            row("gpl.vsd", 'g', "gpl-3.0"),
            row("no-license.vsd", 'n', "wtfpl"),
            row("notes.txt", 't', "mit"),
        ];
        let plan = plan(&rows);
        assert_eq!(plan.rows, 5);
        assert_eq!(plan.duplicates, 1);
        assert_eq!(plan.no_tier, 1);
        assert_eq!(plan.no_ext, 1);
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.files[0].blob_id, "a".repeat(40));
        assert_eq!(plan.files[0].tier, Tier::A);
        assert_eq!(plan.files[1].tier, Tier::B);
    }

    #[test]
    fn computes_git_blob_sha1() {
        // `git hash-object` on the empty blob and on "hello\n".
        assert_eq!(
            git_blob_sha1(b""),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(
            git_blob_sha1(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }

    #[test]
    fn builds_raw_urls() {
        assert_eq!(
            raw_url("o/r", "refs/heads/master", "src/a.vsd"),
            "https://raw.githubusercontent.com/o/r/master/src/a.vsd"
        );
        assert_eq!(
            raw_url("o/r", "refs/tags/v1", "a.vsd"),
            "https://raw.githubusercontent.com/o/r/v1/a.vsd"
        );
        // spaces and non-ASCII are percent-encoded per segment
        assert_eq!(
            raw_url("o/r", "refs/heads/master", "rocketmq/RocketMQ 设计图.vsd"),
            "https://raw.githubusercontent.com/o/r/master/rocketmq/RocketMQ%20%E8%AE%BE%E8%AE%A1%E5%9B%BE.vsd"
        );
    }

    #[test]
    fn parses_ls_remote_output() {
        assert_eq!(
            parse_ls_remote("1234567890abcdef1234567890abcdef12345678\trefs/heads/master\n")
                .unwrap(),
            "1234567890abcdef1234567890abcdef12345678"
        );
        assert!(parse_ls_remote("").is_err());
        assert!(parse_ls_remote("not-a-sha\trefs/heads/master\n").is_err());
    }

    #[test]
    fn round_trips_state_rows() {
        let row = StateRow {
            blob_id: "a".repeat(40),
            tier: "a".to_owned(),
            license: "mit".to_owned(),
            repo: "o/r".to_owned(),
            commit: "c".to_owned(),
            path: "x, y.vsd".to_owned(),
            ext: "vsd".to_owned(),
            sha256: "s".to_owned(),
            size: 42,
            status: "kept".to_owned(),
            note: String::new(),
        };
        let text = format!("{}\n{}\n", state_header(), row.line());
        let rows = state_rows(&text).unwrap();
        assert_eq!(rows, vec![row]);
        // a header-only file is empty, a malformed row is an error
        assert!(state_rows(&state_header()).unwrap().is_empty());
        assert!(state_rows("blob_id,tier\n").is_err());
    }

    #[test]
    fn reads_pins() {
        let pins = read_pins("o/r\trefs/heads/master\tabc\nbad line\n");
        assert_eq!(
            pins.get(&("o/r".to_owned(), "refs/heads/master".to_owned())),
            Some(&"abc".to_owned())
        );
        assert_eq!(pins.len(), 1);
    }

    // ---- fetch -----------------------------------------------------

    /// A [`ScoutNet`] that serves canned bodies and counts calls.
    struct FakeNet {
        bodies: BTreeMap<String, std::result::Result<Vec<u8>, NetError>>,
        pins: BTreeMap<String, std::result::Result<String, String>>,
        gets: AtomicUsize,
        pin_calls: AtomicUsize,
    }

    impl FakeNet {
        fn new() -> Self {
            Self {
                bodies: BTreeMap::new(),
                pins: BTreeMap::new(),
                gets: AtomicUsize::new(0),
                pin_calls: AtomicUsize::new(0),
            }
        }
    }

    impl ScoutNet for FakeNet {
        fn get(&self, url: &str) -> std::result::Result<Vec<u8>, NetError> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            match self.bodies.get(url) {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(e)) => Err(e.clone()),
                None => Err(NetError::Failed(format!("no fake body for {url}"))),
            }
        }

        fn pin(&self, repo: &str, _git_ref: &str) -> Result<String> {
            self.pin_calls.fetch_add(1, Ordering::SeqCst);
            match self.pins.get(repo) {
                Some(Ok(commit)) => Ok(commit.clone()),
                Some(Err(e)) => Err(e.clone()),
                None => Err("no fake pin".into()),
            }
        }
    }

    /// A one-blob plan whose blob id matches `bytes`.
    fn one_blob_plan(bytes: &[u8], ext: &str) -> (Plan, Planned) {
        let mut planned = Planned {
            repo: "o/r".to_owned(),
            git_ref: "refs/heads/master".to_owned(),
            path: format!("a.{ext}"),
            blob_id: git_blob_sha1(bytes),
            license: "mit".to_owned(),
            tier: Tier::A,
            ext: ext_of(ext).unwrap(),
        };
        planned.blob_id = git_blob_sha1(bytes);
        (
            Plan {
                files: vec![planned.clone()],
                rows: 1,
                ..Plan::default()
            },
            planned,
        )
    }

    #[test]
    fn fetch_verifies_and_keeps_a_good_file() {
        let t = TempDir::new("scout-fetch");
        let (plan, planned) = one_blob_plan(b"vsd bytes", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies.insert(url, Ok(b"vsd bytes".to_vec()));
        net.pins.insert("o/r".to_owned(), Ok("feedface".repeat(5)));

        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.unpinned, 0);
        let dest = file_path(t.path(), &planned);
        assert_eq!(std::fs::read(&dest).unwrap(), b"vsd bytes");
        let rows = state_rows(&std::fs::read_to_string(t.join(FETCH_CSV)).unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "kept");
        assert_eq!(rows[0].size, 9);
        assert_eq!(rows[0].sha256, sha256_hex(b"vsd bytes"));
        assert_eq!(rows[0].commit, "feedface".repeat(5));
    }

    #[test]
    fn fetch_drops_a_file_that_does_not_verify() {
        let t = TempDir::new("scout-mismatch");
        let (plan, planned) = one_blob_plan(b"expected bytes", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies.insert(url, Ok(b"other bytes".to_vec()));

        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.mismatch, 1);
        assert!(!file_path(t.path(), &planned).exists());
        let rows = state_rows(&std::fs::read_to_string(t.join(FETCH_CSV)).unwrap()).unwrap();
        assert_eq!(rows[0].status, "mismatch");
        assert_eq!(net.pin_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn fetch_enforces_the_size_cap() {
        let t = TempDir::new("scout-toobig");
        let (plan, planned) = one_blob_plan(b"expected", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies
            .insert(url, Err(NetError::TooBig(100 * 1024 * 1024)));

        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.too_big, 1);
        let rows = state_rows(&std::fs::read_to_string(t.join(FETCH_CSV)).unwrap()).unwrap();
        assert_eq!(rows[0].status, "too-big");
    }

    #[test]
    fn fetch_retries_and_records_failures() {
        let t = TempDir::new("scout-failed");
        let (plan, planned) = one_blob_plan(b"expected", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies
            .insert(url, Err(NetError::Failed("boom".to_owned())));

        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.failed, 1);
        // the run retried before giving up
        assert_eq!(net.gets.load(Ordering::SeqCst), RETRIES + 1);
        let rows = state_rows(&std::fs::read_to_string(t.join(FETCH_CSV)).unwrap()).unwrap();
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[0].note, "boom");
    }

    #[test]
    fn fetch_resumes_and_never_refetches_a_kept_file() {
        let t = TempDir::new("scout-resume");
        let (plan, planned) = one_blob_plan(b"vsd bytes", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies.insert(url.clone(), Ok(b"vsd bytes".to_vec()));
        net.pins.insert("o/r".to_owned(), Ok("c".repeat(40)));

        run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.kept, 0);
        assert_eq!(summary.present, 1);
        assert_eq!(net.gets.load(Ordering::SeqCst), 1, "no second download");

        // corrupt the file: the next run refetches it
        std::fs::write(file_path(t.path(), &planned), b"corrupt").unwrap();
        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.kept, 1);
        assert_eq!(net.gets.load(Ordering::SeqCst), 2);

        // a permanent drop is never retried
        let (plan2, _) = one_blob_plan(b"other expected", "vsd");
        let mut planned2 = plan2.files[0].clone();
        planned2.blob_id = "b".repeat(40);
        let plan2 = Plan {
            files: vec![planned2.clone()],
            rows: 1,
            ..Plan::default()
        };
        let url2 = raw_url(&planned2.repo, &planned2.git_ref, &planned2.path);
        net.bodies.insert(url2, Ok(b"mismatching".to_vec()));
        run_fetch(&net, t.path(), &plan2, 1, 0).unwrap();
        let gets = net.gets.load(Ordering::SeqCst);
        let summary = run_fetch(&net, t.path(), &plan2, 1, 0).unwrap();
        assert_eq!(summary.mismatch, 0);
        assert_eq!(net.gets.load(Ordering::SeqCst), gets);
    }

    #[test]
    fn fetch_keeps_an_unpinned_file() {
        let t = TempDir::new("scout-unpinned");
        let (plan, planned) = one_blob_plan(b"vsd bytes", "vsd");
        let mut net = FakeNet::new();
        let url = raw_url(&planned.repo, &planned.git_ref, &planned.path);
        net.bodies.insert(url, Ok(b"vsd bytes".to_vec()));
        net.pins
            .insert("o/r".to_owned(), Err("repo is gone".to_owned()));

        let summary = run_fetch(&net, t.path(), &plan, 1, 0).unwrap();
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.unpinned, 1);
        let rows = state_rows(&std::fs::read_to_string(t.join(FETCH_CSV)).unwrap()).unwrap();
        assert_eq!(rows[0].status, "kept");
        assert!(rows[0].commit.is_empty());
        assert!(rows[0].note.contains("unpinned"));

        // the pins cache keeps the good pins across runs
        let mut net2 = FakeNet::new();
        net2.bodies.insert(
            raw_url("o/r", "refs/heads/master", "a.vsd"),
            Ok(b"vsd bytes".to_vec()),
        );
        net2.pins.insert("o/r".to_owned(), Ok("d".repeat(40)));
        let (plan2, _) = one_blob_plan(b"more", "vsd");
        let mut planned2 = plan2.files[0].clone();
        planned2.blob_id = git_blob_sha1(b"vsd bytes");
        let plan2 = Plan {
            files: vec![planned2],
            rows: 1,
            ..Plan::default()
        };
        // same repo + ref: the cached pin is reused, no ls-remote call
        let summary = run_fetch(&net2, t.path(), &plan2, 1, 0).unwrap();
        assert_eq!(summary.present, 1);
        assert_eq!(net2.pin_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_net_reports_http_errors_and_small_bodies() {
        // A local listener: one 200 with a small body, one 404.
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for response in [
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd".to_vec(),
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            ] {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let _ = stream.write_all(&response);
                    let _ = stream.flush();
                }
            }
        });
        let net = LiveNet::new();
        let base = format!("http://{addr}");
        assert_eq!(net.get(&format!("{base}/a")).unwrap(), b"abcd");
        match net.get(&format!("{base}/missing")) {
            Err(NetError::Failed(why)) => assert!(why.contains("404"), "{why}"),
            other => panic!("expected an error, got {other:?}"),
        }
        handle.join().unwrap();
    }

    #[test]
    fn live_net_pins_with_git_ls_remote() {
        // A local bare repository with one branch.
        let t = TempDir::new("scout-pin");
        let repo = t.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for cmd in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        ] {
            let status = Command::new("git")
                .args(&cmd)
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&out.stdout).trim().to_owned();

        // `pin` speaks https://github.com/<repo>; point the test at the
        // local path through the parse, which is what `pin` adds.
        assert_eq!(
            parse_ls_remote(&format!("{head}\trefs/heads/main\n")).unwrap(),
            head
        );
    }

    // ---- classify --------------------------------------------------

    /// Builds an OPC package with the given entries.
    fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write as _;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, body) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    const CONTENT_TYPES_DRAWING: &str = r#"<?xml version="1.0"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Override PartName="/visio/document.xml" ContentType="application/vnd.ms-visio.drawing.main+xml"/>
</Types>"#;

    #[test]
    fn classifies_an_opc_drawing() {
        let bytes = make_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES_DRAWING),
            ("visio/document.xml", "<VisioDocument/>"),
        ]);
        let facts = classify_bytes(&bytes, FileExt::Vsdx, "chart.vsdx");
        assert_eq!(facts.kind, "ooxml-drawing");
        assert_eq!(facts.kind_matches_ext, "yes");
        assert!(!facts.ms_author);
        assert_eq!(facts.excluded_reason(), None);
    }

    #[test]
    fn classifies_a_stencil_whose_kind_matches_the_extension() {
        let bytes = make_zip(&[
            (
                "[Content_Types].xml",
                r#"<Types><Override PartName="/visio/document.xml"
                    ContentType="application/vnd.ms-visio.stencil.main+xml"/></Types>"#,
            ),
            ("visio/document.xml", "<VisioDocument/>"),
        ]);
        let facts = classify_bytes(&bytes, FileExt::Vssx, "mine.vssx");
        assert_eq!(facts.kind, "ooxml-stencil");
        assert_eq!(facts.kind_matches_ext, "yes");
        let facts = classify_bytes(&bytes, FileExt::Vsdx, "misnamed.vsdx");
        assert_eq!(facts.kind_matches_ext, "no");
    }

    #[test]
    fn a_zip_without_a_visio_part_is_other() {
        let bytes = make_zip(&[("readme.txt", "hello")]);
        let facts = classify_bytes(&bytes, FileExt::Vsdx, "not-visio.vsdx");
        assert_eq!(facts.kind, "zip-other");
        assert_eq!(facts.kind_matches_ext, "unknown");
    }

    #[test]
    fn the_filter_sees_the_document_properties() {
        let bytes = make_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES_DRAWING),
            ("visio/document.xml", "<VisioDocument/>"),
            (
                "docProps/core.xml",
                r#"<coreProperties xmlns:dc="x">
                     <dc:creator>Microsoft Corporation</dc:creator>
                     <dc:title>My Drawing</dc:title>
                   </coreProperties>"#,
            ),
            (
                "docProps/app.xml",
                r#"<Properties><Company>Contoso</Company>
                     <Template>BASICD_M.VSTX</Template></Properties>"#,
            ),
        ]);
        let facts = classify_bytes(&bytes, FileExt::Vsdx, "chart.vsdx");
        assert!(facts.ms_author);
        assert!(!facts.ms_company);
        assert!(
            facts.template_origin,
            "its masters come from a stock template"
        );
        assert_eq!(facts.excluded_reason(), Some("microsoft-author"));
    }

    #[test]
    fn a_copy_of_a_stock_template_is_excluded_by_name() {
        let bytes = make_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES_DRAWING),
            ("visio/document.xml", "<VisioDocument/>"),
        ]);
        let facts = classify_bytes(&bytes, FileExt::Vsdx, "basicd_m.vsdx");
        assert!(facts.template_copy);
        assert_eq!(facts.excluded_reason(), Some("template-copy"));

        let facts = classify_bytes(&bytes, FileExt::Vsdx, "my-flowchart.vsdx");
        assert!(!facts.template_copy);
    }

    #[test]
    fn stock_template_names_are_short_codes_with_an_m_suffix() {
        assert!(is_stock_template_name("BASICD_M.VSTX"));
        assert!(is_stock_template_name("basflo_m.vss"));
        assert!(is_stock_template_name("XFUNC_M"));
        assert!(is_stock_template_name("orgchrt_m.vstx"));
        assert!(!is_stock_template_name("my_drawing.vsdx"));
        assert!(!is_stock_template_name("basicd.vsdx"));
        assert!(!is_stock_template_name("averyveryverylongcode_m.vsdx"));
    }

    #[test]
    fn classifies_ole_xml_and_garbage() {
        let ole = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 0, 0];
        assert_eq!(classify_bytes(&ole, FileExt::Vsd, "a.vsd").kind, "ole");
        assert_eq!(
            classify_bytes(b"<?xml version=\"1.0\"?><Vdx/>", FileExt::Vdx, "a.vdx").kind,
            "vdx"
        );
        assert_eq!(
            classify_bytes(b"garbage", FileExt::Vdx, "a.vdx").kind,
            "other"
        );
        assert_eq!(
            classify_bytes(b"PK\x03\x04garbage", FileExt::Vsdx, "a.vsdx").kind,
            "zip-invalid"
        );
    }

    #[test]
    fn reads_xml_pieces() {
        assert_eq!(
            override_type(
                r#"<Types><Override PartName="/visio/document.xml" ContentType="x/y"/></Types>"#,
                "/visio/document.xml"
            ),
            Some("x/y".to_owned())
        );
        assert_eq!(override_type("<Types/>", "/visio/document.xml"), None);
        assert_eq!(
            tag_text(
                r#"<p:Properties><p:Company>Contoso</p:Company></p:Properties>"#,
                "Company"
            ),
            Some("Contoso".to_owned())
        );
        assert_eq!(tag_text("<p/>", "Company"), None);
    }

    #[test]
    fn parses_ole_facts() {
        let text = format!(
            "{}\n{},ole-stencil,11,1,0,0,0\n",
            ole_facts_header(),
            "a".repeat(40)
        );
        let rows = ole_facts_rows(&text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "ole-stencil");
        assert_eq!(rows[0].version, "11");
        assert!(rows[0].ms_author);
        // a header-only file is empty, a malformed row is an error
        assert!(ole_facts_rows(&ole_facts_header()).unwrap().is_empty());
        assert!(ole_facts_rows(&format!("{}\nx,y\n", ole_facts_header())).is_err());
    }

    #[test]
    fn classify_writes_the_index_and_the_exclusions() {
        let t = TempDir::new("scout-classify");
        let out = t.path();
        std::fs::create_dir_all(out.join("a")).unwrap();
        let drawing = make_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES_DRAWING),
            ("visio/document.xml", "<VisioDocument/>"),
        ]);
        let ole = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 9, 9];
        let good = "1".repeat(40);
        let ms = "2".repeat(40);
        let vsd = "3".repeat(40);
        std::fs::write(out.join(format!("a/{good}.vsdx")), &drawing).unwrap();
        std::fs::write(out.join(format!("a/{ms}.vsd")), ole).unwrap();
        std::fs::write(out.join(format!("a/{vsd}.vsd")), ole).unwrap();

        let mut state = format!("{}\n", state_header());
        for (blob, ext) in [(&good, "vsdx"), (&ms, "vsd"), (&vsd, "vsd")] {
            let row = StateRow {
                blob_id: (*blob).clone(),
                tier: "a".to_owned(),
                license: "mit".to_owned(),
                repo: "o/r".to_owned(),
                commit: "c".to_owned(),
                path: format!("p/{blob}.{ext}"),
                ext: (*ext).to_owned(),
                sha256: "s".to_owned(),
                size: 1,
                status: "kept".to_owned(),
                note: String::new(),
            };
            state.push_str(&row.line());
            state.push('\n');
        }
        std::fs::write(out.join(FETCH_CSV), state).unwrap();
        let ole_facts = format!(
            "{}\n{ms},ole-template,11,0,0,1,0\n{vsd},ole-drawing,6,0,0,0,0\n",
            ole_facts_header()
        );
        std::fs::write(out.join(OLE_FACTS_CSV), ole_facts).unwrap();

        // the OLE facts say `{ms}` is a template copy: excluded. The
        // zip scan sees nothing Microsoft about its bytes.
        let summary = run_classify(out).unwrap();
        assert_eq!(summary.files, 3);
        assert_eq!(summary.excluded, 1);
        assert_eq!(summary.ole_missing, 0);

        let index = std::fs::read_to_string(out.join(INDEX_CSV)).unwrap();
        assert!(index.starts_with(&index_header()));
        assert!(index.contains(&good));
        assert!(!index.contains(&ms), "an excluded file is not in the index");
        let excluded = std::fs::read_to_string(out.join(EXCLUDED_CSV)).unwrap();
        assert!(excluded.contains(&ms));
        assert!(excluded.contains("template-copy"));
        assert!(excluded.contains("ole-template"));
    }

    #[test]
    fn classify_needs_the_ole_facts_of_ole_files() {
        let t = TempDir::new("scout-classify-missing");
        let out = t.path();
        std::fs::create_dir_all(out.join("a")).unwrap();
        let ole = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1, 1];
        let vsd = "4".repeat(40);
        std::fs::write(out.join(format!("a/{vsd}.vsd")), ole).unwrap();
        let row = StateRow {
            blob_id: vsd.clone(),
            tier: "a".to_owned(),
            license: "mit".to_owned(),
            repo: "o/r".to_owned(),
            commit: String::new(),
            path: "p/a.vsd".to_owned(),
            ext: "vsd".to_owned(),
            sha256: String::new(),
            size: 1,
            status: "kept".to_owned(),
            note: String::new(),
        };
        std::fs::write(
            out.join(FETCH_CSV),
            format!("{}\n{}\n", state_header(), row.line()),
        )
        .unwrap();

        let summary = run_classify(out).unwrap();
        assert_eq!(summary.ole_missing, 1);
        let index = std::fs::read_to_string(out.join(INDEX_CSV)).unwrap();
        assert!(index.contains("ole"));
    }
}
