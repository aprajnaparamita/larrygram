//! `cargo xtask corpus`: fetch and verify the third-party test corpus (ADR 0005).
//!
//! Reads `corpus/manifest.toml`, then for each `[[file]]` entry either confirms
//! the file at `corpus/<name>` is already present with the right size and
//! sha256, or fetches it (from the source's `mirror_raw` URL, or from a local
//! directory in `--offline` mode). Files are written atomically (temp file +
//! rename) and a sha256 mismatch is an error that never keeps the file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::util::Result;

/// Parsed command-line options for the corpus task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Do not download; only report missing or mismatched files.
    pub verify_only: bool,
    /// Optional local directory to copy files from instead of the network.
    pub offline: Option<PathBuf>,
}

/// Parses the arguments after `cargo xtask corpus`.
pub fn parse_args(args: &[String]) -> Result<Options> {
    let mut verify_only = false;
    let mut offline = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--verify-only" => verify_only = true,
            "--offline" => {
                let dir = it.next().ok_or("--offline needs a directory")?;
                offline = Some(PathBuf::from(dir));
            }
            other => return Err(format!("unknown corpus argument `{other}`")),
        }
    }
    Ok(Options {
        verify_only,
        offline,
    })
}

/// A parsed corpus manifest: the `[sources.*]` table and the `[[file]]` array.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    /// Source name → source definition.
    pub sources: BTreeMap<String, Source>,
    /// Files to fetch, in manifest order.
    pub files: Vec<FileEntry>,
}

/// One `[sources.<name>]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Raw-file URL template with `{commit}` and `{path}` placeholders.
    pub mirror_raw: String,
    /// Pinned upstream commit substituted for `{commit}`.
    pub commit: String,
}

/// One `[[file]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative path inside `corpus/` (e.g. `vsd/dwg.vsd`).
    pub name: String,
    /// Key into `sources`.
    pub source: String,
    /// Upstream path inside the source tree.
    pub path: String,
    /// Expected lowercase hex sha256.
    pub sha256: String,
    /// Expected byte size.
    pub size: u64,
}

/// Result of checking an on-disk file against an expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// The file does not exist.
    Missing,
    /// The file exists with the right size and sha256.
    Valid,
    /// The file exists but has the wrong size or sha256.
    Mismatch(String),
}

/// Parses `corpus/manifest.toml`.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("bad corpus/manifest.toml: {e}"))?;

    let mut sources = BTreeMap::new();
    if let Some(srcs) = table.get("sources").and_then(|s| s.as_table()) {
        for (name, val) in srcs {
            let where_ = format!("sources.{name}");
            let t = val
                .as_table()
                .ok_or_else(|| format!("manifest: {where_} is not a table"))?;
            let mirror_raw = get_str(t, "mirror_raw", &where_)?.to_owned();
            let commit = get_str(t, "commit", &where_)?.to_owned();
            sources.insert(name.clone(), Source { mirror_raw, commit });
        }
    }

    let mut files = Vec::new();
    if let Some(entries) = table.get("file").and_then(|f| f.as_array()) {
        for (i, val) in entries.iter().enumerate() {
            let where_ = format!("file[{i}]");
            let t = val
                .as_table()
                .ok_or_else(|| format!("manifest: {where_} is not a table"))?;
            let name = get_str(t, "name", &where_)?.to_owned();
            let source = get_str(t, "source", &where_)?.to_owned();
            let path = get_str(t, "path", &where_)?.to_owned();
            let sha256 = get_str(t, "sha256", &where_)?.to_owned();
            if !is_valid_sha256(&sha256) {
                return Err(format!("manifest: {where_}.sha256 is not 64 hex digits"));
            }
            let size = get_int(t, "size", &where_)?;
            files.push(FileEntry {
                name,
                source,
                path,
                sha256,
                size,
            });
        }
    }

    Ok(Manifest { sources, files })
}

fn get_str<'a>(t: &'a toml::Table, key: &str, where_: &str) -> Result<&'a str> {
    t.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("manifest: {where_} has no string `{key}`"))
}

fn get_int(t: &toml::Table, key: &str, where_: &str) -> Result<u64> {
    let v = t
        .get(key)
        .ok_or_else(|| format!("manifest: {where_} has no `{key}`"))?;
    let i = v
        .as_integer()
        .ok_or_else(|| format!("manifest: {where_}.{key} is not an integer"))?;
    u64::try_from(i).map_err(|_| format!("manifest: {where_}.{key} is negative"))
}

fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Rejects file names that could escape `corpus/`: empty, absolute, or
/// containing `..` (or any non-normal component).
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err("empty file name".into());
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(format!("`{name}` must be a relative path"));
    }
    for comp in path.components() {
        if !matches!(comp, Component::Normal(_)) {
            return Err(format!(
                "`{name}` must not contain `..` or absolute components"
            ));
        }
    }
    Ok(())
}

/// Substitutes `{commit}` and `{path}` in a `mirror_raw` template.
pub fn template_url(mirror_raw: &str, commit: &str, path: &str) -> Result<String> {
    let url = mirror_raw
        .replace("{commit}", commit)
        .replace("{path}", path);
    if url.contains('{') || url.contains('}') {
        return Err(format!(
            "unresolved placeholder in URL template: {mirror_raw}"
        ));
    }
    Ok(url)
}

/// Computes the lowercase hex sha256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks `path` against the expected size and sha256 without touching the
/// network. Returns `Ok(Missing)` when the file does not exist.
pub fn file_state(path: &Path, expected_sha: &str, expected_size: u64) -> Result<FileState> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(e) => return Err(format!("cannot stat {}: {e}", path.display())),
    };
    let actual = meta.len();
    if actual != expected_size {
        return Ok(FileState::Mismatch(format!(
            "size {actual} != expected {expected_size}"
        )));
    }
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let digest = sha256_hex(&bytes);
    if digest == expected_sha {
        Ok(FileState::Valid)
    } else {
        Ok(FileState::Mismatch(format!(
            "sha256 {digest} != expected {expected_sha}"
        )))
    }
}

/// Copies the file named by `path` (basename) from `offline_dir` to `dest`,
/// atomically.
pub fn copy_offline(offline_dir: &Path, path: &str, dest: &Path) -> Result<()> {
    let file_name = Path::new(path)
        .file_name()
        .ok_or_else(|| format!("`{path}` has no file name"))?;
    let src = offline_dir.join(file_name);
    if !src.is_file() {
        return Err(format!("offline: {} does not exist", src.display()));
    }
    let parent = parent_of(dest);
    fs::create_dir_all(&parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let tmp = crate::util::unique_temp_path(dest)?;
    fs::copy(&src, &tmp)
        .map_err(|e| format!("cannot copy {} -> {}: {e}", src.display(), tmp.display()))?;
    crate::util::finalize(&tmp, dest)
}

/// A shared HTTP agent using rustls with the platform's root certificate store
/// (loaded via `rustls-native-certs`, so we do not need the CDLA-licensed
/// webpki-roots crate).
fn http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(platform_roots())
            .build();
        ureq::Agent::config_builder().tls_config(tls).build().into()
    })
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

/// Downloads `entry` from `source`'s raw mirror and writes it atomically.
pub fn download(source: &Source, entry: &FileEntry, dest: &Path) -> Result<()> {
    download_with(http_agent(), source, entry, dest)
}

/// [`download`] with an injectable HTTP agent (C1): the tests drive it
/// against a local listener; the production path passes
/// [`http_agent`] and behaves exactly as before.
fn download_with(
    agent: &ureq::Agent,
    source: &Source,
    entry: &FileEntry,
    dest: &Path,
) -> Result<()> {
    let url = template_url(&source.mirror_raw, &source.commit, &entry.path)?;
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    if response.status() != 200 {
        return Err(format!("GET {url}: HTTP {}", response.status()));
    }
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| format!("GET {url}: read body: {e}"))?;
    write_atomic(dest, &body)
}

/// Writes `bytes` to `dest` atomically (temp file + rename). The one
/// atomic writer shared with the extractors
/// ([`crate::util::write_atomic_bytes`]); temp names are unique per
/// call, so concurrent `cargo xtask corpus` runs never collide.
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    crate::util::write_atomic_bytes(dest, bytes)
}

fn parent_of(dest: &Path) -> PathBuf {
    dest.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Runs the corpus task with already-parsed options.
pub fn run_opts(opts: &Options) -> Result<()> {
    run_with_root(opts, &crate::util::root())
}

/// Core of the corpus task, separated so tests can point it at a
/// temp root.
pub fn run_with_root(opts: &Options, root: &Path) -> Result<()> {
    let mut fetch = |source: &Source, entry: &FileEntry, dest: &Path| match &opts.offline {
        Some(dir) => copy_offline(dir, &entry.path, dest),
        None => download(source, entry, dest),
    };
    run_with_fetch(opts, root, &mut fetch)
}

/// The fetch loop with the fetch step injected, so the post-fetch
/// verification arms are testable without a network. The production
/// fetchers are [`download`] and [`copy_offline`] (see
/// [`run_with_root`]).
fn run_with_fetch(
    opts: &Options,
    root: &Path,
    fetch: &mut dyn FnMut(&Source, &FileEntry, &Path) -> Result<()>,
) -> Result<()> {
    let manifest_path = root.join("corpus").join("manifest.toml");
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest = parse_manifest(&text)?;
    let corpus_dir = root.join("corpus");

    let mut fetched = 0usize;
    let mut present = 0usize;
    let mut failures = Vec::new();

    for entry in &manifest.files {
        if let Err(e) = validate_name(&entry.name) {
            failures.push(format!("{}: {e}", entry.name));
            continue;
        }
        let source = match manifest.sources.get(&entry.source) {
            Some(s) => s,
            None => {
                failures.push(format!("{}: unknown source `{}`", entry.name, entry.source));
                continue;
            }
        };
        let dest = corpus_dir.join(&entry.name);
        match file_state(&dest, &entry.sha256, entry.size) {
            Ok(FileState::Valid) => present += 1,
            Ok(FileState::Missing) | Ok(FileState::Mismatch(_)) => {
                if opts.verify_only {
                    failures.push(format!("{}: missing or mismatched", entry.name));
                    continue;
                }
                match fetch(source, entry, &dest) {
                    Ok(()) => match file_state(&dest, &entry.sha256, entry.size) {
                        Ok(FileState::Valid) => fetched += 1,
                        Ok(FileState::Missing) => {
                            failures.push(format!("{}: fetch produced no file", entry.name))
                        }
                        Ok(FileState::Mismatch(m)) => {
                            let _ = fs::remove_file(&dest);
                            failures.push(format!("{}: {m}", entry.name));
                        }
                        Err(e) => failures.push(format!("{}: {e}", entry.name)),
                    },
                    Err(e) => failures.push(format!("{}: {e}", entry.name)),
                }
            }
            Err(e) => failures.push(format!("{}: {e}", entry.name)),
        }
    }

    println!(
        "corpus: fetched {fetched}, already present {present}, failed {}",
        failures.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "corpus: {} file(s) failed:\n  {}",
            failures.len(),
            failures.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_support::TempDir;

    const SAMPLE: &str = r#"[sources.libvisio]
repo = "https://git.libreoffice.org/libvisio"
mirror_raw = "https://raw.githubusercontent.com/LibreOffice/libvisio/{commit}/{path}"
commit = "c7c970775b750a4bf7fd1b4364d69bac9815bfcb"
license = "MPL-2.0"

[[file]]
name = "vsd/dwg.vsd"
source = "libvisio"
path = "src/test/data/dwg.vsd"
sha256 = "4d2c343313352067a6a709360fe146d6a7a1d22e8c42873de45011d0ee51a3c1"
size = 41984
"#;

    #[test]
    fn parses_manifest() {
        let m = parse_manifest(SAMPLE).unwrap();
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.sources.len(), 1);
        let src = &m.sources["libvisio"];
        assert_eq!(src.commit, "c7c970775b750a4bf7fd1b4364d69bac9815bfcb");
        assert!(src.mirror_raw.contains("{commit}"));
        let f = &m.files[0];
        assert_eq!(f.name, "vsd/dwg.vsd");
        assert_eq!(f.source, "libvisio");
        assert_eq!(f.path, "src/test/data/dwg.vsd");
        assert_eq!(f.size, 41984);
    }

    #[test]
    fn rejects_malformed_manifest() {
        assert!(parse_manifest("not toml").is_err());
        // missing sha256
        assert!(
            parse_manifest("[[file]]\nname = \"a\"\nsource = \"s\"\npath = \"p\"\nsize = 1\n")
                .is_err()
        );
        // bad sha256
        assert!(
            parse_manifest(
                "[[file]]\nname = \"a\"\nsource = \"s\"\npath = \"p\"\nsha256 = \"xyz\"\nsize = 1\n"
            )
            .is_err()
        );
        // negative size
        assert!(parse_manifest(
            "[[file]]\nname = \"a\"\nsource = \"s\"\npath = \"p\"\nsha256 = \"4d2c343313352067a6a709360fe146d6a7a1d22e8c42873de45011d0ee51a3c1\"\nsize = -1\n"
        )
        .is_err());
        // non-string source field
        assert!(parse_manifest("[sources.s]\nmirror_raw = 3\ncommit = \"c\"\n").is_err());
    }

    #[test]
    fn validates_names() {
        assert!(validate_name("vsd/dwg.vsd").is_ok());
        assert!(validate_name("a/b/c").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("/etc/passwd").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name("a/../../b").is_err());
        assert!(validate_name("a/../b").is_err());
        // "." components are harmless: `Path::components` normalizes them away.
        assert!(validate_name("a/./b").is_ok());
    }

    #[test]
    fn templates_urls() {
        let src = &parse_manifest(SAMPLE).unwrap().sources["libvisio"];
        let url = template_url(&src.mirror_raw, &src.commit, "src/test/data/dwg.vsd").unwrap();
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/LibreOffice/libvisio/c7c970775b750a4bf7fd1b4364d69bac9815bfcb/src/test/data/dwg.vsd"
        );
        // unknown placeholders are rejected
        assert!(template_url("https://x/{commit}/{foo}", "c", "p").is_err());
    }

    #[test]
    fn verifies_files() {
        let t = TempDir::new("corpus");
        let data = b"hello, corpus\n";
        let expected_sha = sha256_hex(data);

        // missing
        assert_eq!(
            file_state(&t.path().join("nope"), &expected_sha, data.len() as u64).unwrap(),
            FileState::Missing
        );

        let f = t.path().join("ok.bin");
        fs::write(&f, data).unwrap();
        assert_eq!(
            file_state(&f, &expected_sha, data.len() as u64).unwrap(),
            FileState::Valid
        );

        // wrong size
        assert!(matches!(
            file_state(&f, &expected_sha, 999).unwrap(),
            FileState::Mismatch(_)
        ));

        // wrong sha
        let bad = format!("a{}", &expected_sha[1..]);
        assert!(matches!(
            file_state(&f, &bad, data.len() as u64).unwrap(),
            FileState::Mismatch(_)
        ));
    }

    #[test]
    fn parses_args() {
        assert_eq!(
            parse_args(&[]).unwrap(),
            Options {
                verify_only: false,
                offline: None
            }
        );
        assert_eq!(
            parse_args(&["--verify-only".to_owned()]).unwrap(),
            Options {
                verify_only: true,
                offline: None
            }
        );
        assert_eq!(
            parse_args(&["--offline".to_owned(), "/x".to_owned()]).unwrap(),
            Options {
                verify_only: false,
                offline: Some("/x".into())
            }
        );
        assert!(parse_args(&["--offline".to_owned()]).is_err());
        assert!(parse_args(&["--bogus".to_owned()]).is_err());
    }

    /// Builds a temp "workspace" with corpus/manifest.toml, an offline dir with
    /// the file, and runs the fetch end-to-end without network.
    #[test]
    fn offline_run_fetches_and_is_idempotent() {
        let root = TempDir::new("corpus");
        let data = b"offline corpus bytes";
        let sha = sha256_hex(data);

        let offline = root.path().join("candidates");
        fs::create_dir_all(&offline).unwrap();
        fs::write(offline.join("dwg.vsd"), data).unwrap();

        let manifest = format!(
            "[sources.libvisio]\nmirror_raw = \"https://x/{{commit}}/{{path}}\"\ncommit = \"c\"\n\n[[file]]\nname = \"vsd/dwg.vsd\"\nsource = \"libvisio\"\npath = \"src/test/data/dwg.vsd\"\nsha256 = \"{sha}\"\nsize = {}\n",
            data.len()
        );
        let corpus = root.path().join("corpus");
        fs::create_dir_all(&corpus).unwrap();
        fs::write(corpus.join("manifest.toml"), manifest).unwrap();

        let opts = Options {
            verify_only: false,
            offline: Some(offline.clone()),
        };
        run_with_root(&opts, root.path()).unwrap();
        let dest = corpus.join("vsd").join("dwg.vsd");
        assert_eq!(fs::read(&dest).unwrap(), data);

        // second run: already present, nothing fetched
        run_with_root(&opts, root.path()).unwrap();

        // verify-only after fetching: passes
        let verify = Options {
            verify_only: true,
            offline: None,
        };
        run_with_root(&verify, root.path()).unwrap();

        // corrupt the file: verify-only fails
        fs::write(&dest, b"corrupted").unwrap();
        assert!(run_with_root(&verify, root.path()).is_err());
    }

    #[test]
    fn offline_run_rejects_missing_source_file() {
        let root = TempDir::new("corpus");
        let corpus = root.path().join("corpus");
        fs::create_dir_all(&corpus).unwrap();
        fs::write(
            corpus.join("manifest.toml"),
            "[sources.s]\nmirror_raw = \"https://x/{commit}/{path}\"\ncommit = \"c\"\n\n[[file]]\nname = \"a.bin\"\nsource = \"s\"\npath = \"src/a.bin\"\nsha256 = \"4d2c343313352067a6a709360fe146d6a7a1d22e8c42873de45011d0ee51a3c1\"\nsize = 1\n",
        )
        .unwrap();
        let offline = root.path().join("empty");
        fs::create_dir_all(&offline).unwrap();
        let opts = Options {
            verify_only: false,
            offline: Some(offline),
        };
        assert!(run_with_root(&opts, root.path()).is_err());
    }

    // ---- C1: `download` against a local HTTP listener ---------------

    /// Serves one canned HTTP response on 127.0.0.1 (plain HTTP) and
    /// returns `host:port` plus the server thread.
    fn serve_once(response: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        (addr.to_string(), handle)
    }

    /// A minimal HTTP response; `content_length` may lie about the
    /// body length to model a truncated transfer.
    fn http_response(status: &str, body: &[u8], content_length: usize) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn source_at(addr: &str) -> Source {
        Source {
            mirror_raw: format!("http://{addr}/{{commit}}/{{path}}"),
            commit: "c0ffee".into(),
        }
    }

    fn sample_entry() -> FileEntry {
        FileEntry {
            name: "vsd/a.vsd".into(),
            source: "s".into(),
            path: "src/test/data/a.vsd".into(),
            sha256: String::new(),
            size: 0,
        }
    }

    #[test]
    fn download_writes_a_200_response() {
        let body = b"corpus bytes";
        let (addr, handle) = serve_once(http_response("200 OK", body, body.len()));
        let t = TempDir::new("corpus-http-200");
        let dest = t.join("vsd/a.vsd");
        download(&source_at(&addr), &sample_entry(), &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        handle.join().unwrap();
    }

    #[test]
    fn download_reports_a_404() {
        let (addr, handle) = serve_once(http_response("404 Not Found", b"", 0));
        let t = TempDir::new("corpus-http-404");
        let dest = t.join("vsd/a.vsd");
        let err = download(&source_at(&addr), &sample_entry(), &dest).unwrap_err();
        assert!(err.contains("404"), "{err}");
        assert!(!dest.exists(), "a failed download keeps no file");
        handle.join().unwrap();
    }

    #[test]
    fn download_checks_the_http_status() {
        // With an agent that keeps non-2xx responses, `download`'s own
        // status check reports them.
        let (addr, handle) = serve_once(http_response("404 Not Found", b"", 0));
        let t = TempDir::new("corpus-http-status");
        let dest = t.join("vsd/a.vsd");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let err = download_with(&agent, &source_at(&addr), &sample_entry(), &dest).unwrap_err();
        assert!(err.contains("HTTP 404"), "{err}");
        assert!(!dest.exists(), "a failed download keeps no file");
        handle.join().unwrap();
    }

    #[test]
    fn download_reports_a_truncated_body() {
        // Content-Length promises 100 bytes; the body has 5.
        let (addr, handle) = serve_once(http_response("200 OK", b"short", 100));
        let t = TempDir::new("corpus-http-trunc");
        let dest = t.join("vsd/a.vsd");
        let err = download(&source_at(&addr), &sample_entry(), &dest).unwrap_err();
        assert!(err.contains("read body"), "{err}");
        assert!(!dest.exists(), "a truncated download keeps no file");
        handle.join().unwrap();
    }

    // ---- C2: fetch-loop failure arms --------------------------------

    /// A temp workspace whose manifest has exactly one entry.
    fn one_file_root(name: &str, source: &str, sha: &str, size: u64) -> TempDir {
        let root = TempDir::new("corpus-loop");
        let corpus = root.join("corpus");
        fs::create_dir_all(&corpus).unwrap();
        let manifest = format!(
            "[sources.s]\nmirror_raw = \"https://x/{{commit}}/{{path}}\"\ncommit = \"c\"\n\n[[file]]\nname = \"{name}\"\nsource = \"{source}\"\npath = \"src/a.bin\"\nsha256 = \"{sha}\"\nsize = {size}\n"
        );
        fs::write(corpus.join("manifest.toml"), manifest).unwrap();
        root
    }

    fn opts() -> Options {
        Options {
            verify_only: false,
            offline: None,
        }
    }

    #[test]
    fn fetch_loop_reports_an_unknown_source() {
        let sha = sha256_hex(b"abc");
        let root = one_file_root("a.bin", "not-a-source", &sha, 3);
        let mut called = false;
        let mut fetch = |_s: &Source, _e: &FileEntry, _d: &Path| {
            called = true;
            Ok(())
        };
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("unknown source `not-a-source`"), "{err}");
        assert!(!called, "an unknown source must not be fetched");
    }

    #[test]
    fn fetch_loop_reports_a_fetch_that_produced_no_file() {
        let sha = sha256_hex(b"abc");
        let root = one_file_root("a.bin", "s", &sha, 3);
        let mut fetch = |_s: &Source, _e: &FileEntry, _d: &Path| Ok(());
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("fetch produced no file"), "{err}");
    }

    #[test]
    fn fetch_loop_deletes_a_file_with_the_wrong_size() {
        let sha = sha256_hex(b"abc");
        let root = one_file_root("a.bin", "s", &sha, 10);
        let dest = root.join("corpus/a.bin");
        let mut fetch =
            |_s: &Source, _e: &FileEntry, d: &Path| fs::write(d, b"abc").map_err(|e| e.to_string());
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("size 3 != expected 10"), "{err}");
        assert!(!dest.exists(), "the bad file must be deleted");
    }

    #[test]
    fn fetch_loop_deletes_a_file_with_the_wrong_sha256() {
        let sha = sha256_hex(b"the expected bytes");
        let root = one_file_root("a.bin", "s", &sha, 3);
        let dest = root.join("corpus/a.bin");
        let mut fetch =
            |_s: &Source, _e: &FileEntry, d: &Path| fs::write(d, b"abc").map_err(|e| e.to_string());
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("sha256"), "{err}");
        assert!(err.contains(&sha), "{err}");
        assert!(!dest.exists(), "the bad file must be deleted");
    }

    /// A symlink loop cannot be stat'ed, so verification errors. The
    /// loop needs unix symlinks, so the test is unix-only (R9: the
    /// xtask tests compile on the Windows runner too).
    #[cfg(unix)]
    #[test]
    fn fetch_loop_reports_a_stat_error() {
        let sha = sha256_hex(b"abc");
        let root = one_file_root("a.bin", "s", &sha, 3);
        let dest = root.join("corpus/a.bin");
        // A symlink loop cannot be stat'ed, so verification errors.
        std::os::unix::fs::symlink("a.bin", &dest).unwrap();
        let mut fetch = |_s: &Source, _e: &FileEntry, _d: &Path| Ok(());
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("cannot stat"), "{err}");
    }

    /// The fetch "succeeds" but leaves a symlink loop behind. Unix-only
    /// like the other loop test (R9).
    #[cfg(unix)]
    #[test]
    fn fetch_loop_reports_a_stat_error_while_verifying() {
        let sha = sha256_hex(b"abc");
        let root = one_file_root("a.bin", "s", &sha, 3);
        // The fetch "succeeds" but leaves a symlink loop behind.
        let mut fetch = |_s: &Source, _e: &FileEntry, d: &Path| {
            let _ = fs::remove_file(d);
            std::os::unix::fs::symlink("a.bin", d).map_err(|e| e.to_string())
        };
        let err = run_with_fetch(&opts(), root.path(), &mut fetch).unwrap_err();
        assert!(err.contains("cannot stat"), "{err}");
    }

    #[test]
    fn rejects_hostile_manifest_names() {
        let root = TempDir::new("corpus");
        let corpus = root.path().join("corpus");
        fs::create_dir_all(&corpus).unwrap();
        fs::write(
            corpus.join("manifest.toml"),
            "[sources.s]\nmirror_raw = \"https://x/{commit}/{path}\"\ncommit = \"c\"\n\n[[file]]\nname = \"../escape.bin\"\nsource = \"s\"\npath = \"src/a.bin\"\nsha256 = \"4d2c343313352067a6a709360fe146d6a7a1d22e8c42873de45011d0ee51a3c1\"\nsize = 1\n",
        )
        .unwrap();
        let offline = root.path().join("empty");
        fs::create_dir_all(&offline).unwrap();
        let opts = Options {
            verify_only: false,
            offline: Some(offline),
        };
        // The hostile name is rejected, so nothing is fetched; but the failure
        // is reported as an error.
        assert!(run_with_root(&opts, root.path()).is_err());
        assert!(!corpus.join("..").join("escape.bin").exists());
    }
}
