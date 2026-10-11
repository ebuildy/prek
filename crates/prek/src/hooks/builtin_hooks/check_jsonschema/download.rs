//! Remote schema downloads with an on-disk cache, ported from upstream `cachedownloader.py`.
//!
//! Unlike upstream, the cache is read first: a cache file checked less than [`MAX_AGE`] ago
//! (its mtime) is used with no request, so runs work offline. An older entry is revalidated
//! like upstream does on every lookup: the cached copy is kept when its download time is at
//! least the response's `Last-Modified`. Without that header, prek compares the response's
//! `ETag` with the one saved next to the cache file and sends it as `If-None-Match`, so a
//! `304` also keeps the cache (upstream issue #668). With neither header, the cached copy
//! always wins, like upstream. A fresh body must parse before it is cached; a body that does
//! not parse is retried like a failed request.
//!
//! The download time is kept in a sidecar file, because the mtime records the last check:
//! revalidating or falling back touches it, which must not make an old copy look newer than
//! a remote change. When revalidation fails without a `4xx` (network error, `5xx`, a body that
//! does not parse), the stale copy is used with a warning and counts as checked, so offline
//! runs do not retry it every time. A `4xx` stays an error, so a deleted schema is reported.
//!
//! `PREK_JSONSCHEMA_MIRROR` fetches schemastore.org URLs from a mirror, for networks without
//! access to schemastore.org. Only the fetched URL changes: the cache key, `$id` and `$ref`
//! resolution keep the original URL.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_lc_rs::digest::{SHA256, digest};
use reqwest::header::{ETAG, IF_NONE_MATCH, LAST_MODIFIED};
use tokio::runtime::Handle;
use tracing::debug;

use crate::http::REQWEST_CLIENT;
use crate::warn_user;

/// One request plus two retries, like upstream.
const ATTEMPTS: usize = 3;

/// How long a checked cache file is used without a request.
pub(super) const MAX_AGE: Duration = Duration::from_hours(14 * 24);

/// URL prefixes that `PREK_JSONSCHEMA_MIRROR` replaces.
const SCHEMASTORE_PREFIXES: &[&str] = &[
    "https://json.schemastore.org/",
    "https://www.schemastore.org/",
    "http://json.schemastore.org/",
    "http://www.schemastore.org/",
];

/// A mirror of schemastore.org, from `PREK_JSONSCHEMA_MIRROR`.
#[derive(Clone, Debug)]
pub(crate) struct Mirror {
    /// The base URL, without trailing slashes.
    base: String,
}

impl Mirror {
    /// Accepts an absolute http(s) URL.
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        let valid = url::Url::parse(value)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.has_host());
        if !valid {
            return Err(format!(
                "Invalid `PREK_JSONSCHEMA_MIRROR` value `{value}`: expected an absolute http(s) URL"
            ));
        }
        Ok(Self {
            base: value.trim_end_matches('/').to_string(),
        })
    }

    /// The mirror URL for a schemastore.org URL, `None` for other hosts.
    pub(super) fn rewrite(&self, url: &str) -> Option<String> {
        SCHEMASTORE_PREFIXES
            .iter()
            .find_map(|prefix| url.strip_prefix(prefix))
            .map(|rest| format!("{}/{rest}", self.base))
    }
}

pub(super) struct Downloader {
    /// `None` means prek's shared client. It is only touched when a download happens,
    /// because creating it loads TLS certificates, which would slow down local-only runs.
    client: Option<reqwest::Client>,
    /// `None` when `--no-cache` is set.
    cache_dir: Option<PathBuf>,
    mirror: Option<Mirror>,
    runtime: Handle,
}

struct Response {
    status: u16,
    last_modified: Option<String>,
    etag: Option<String>,
    body: Vec<u8>,
}

/// HTTP 304: the cached copy matches the `If-None-Match` `ETag`.
const NOT_MODIFIED: u16 = 304;

/// A cache file and what is known about it.
struct Cached {
    path: PathBuf,
    content: Vec<u8>,
    /// The mtime: when the copy was last downloaded or confirmed.
    checked: SystemTime,
    /// Seconds since the epoch.
    downloaded: u64,
    etag: Option<String>,
}

impl Cached {
    fn read(path: &Path) -> Option<Self> {
        let content = fs_err::read(path).ok()?;
        let checked = fs_err::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()?;
        // Cache files written before the sidecar existed were downloaded at their mtime.
        let downloaded = fs_err::read_to_string(downloaded_path(path))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| epoch_secs(checked));
        Some(Self {
            content,
            checked,
            downloaded,
            etag: fs_err::read_to_string(etag_path(path)).ok(),
            path: path.to_path_buf(),
        })
    }

    fn is_fresh(&self) -> bool {
        SystemTime::now()
            .duration_since(self.checked)
            .is_ok_and(|age| age < MAX_AGE)
    }

    /// Marks the copy as checked now.
    fn touch(&self) {
        let touched = fs_err::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .and_then(|file| file.file().set_modified(SystemTime::now()));
        if let Err(err) = touched {
            debug!("Failed to touch `{}`: {err}", self.path.display());
        }
    }
}

/// Why revalidation or a download failed.
struct Failure {
    message: String,
    /// A `4xx` response: the schema is gone, so a cached copy must not hide it.
    client_error: bool,
}

impl Downloader {
    pub(super) fn new(
        client: Option<reqwest::Client>,
        cache_dir: Option<PathBuf>,
        mirror: Option<Mirror>,
        runtime: Handle,
    ) -> Self {
        Self {
            client,
            cache_dir,
            mirror,
            runtime,
        }
    }

    /// Reads `url` from the cache, or downloads it. `parses` decides whether a fresh body is
    /// usable. This blocks on the runtime, so call it from a blocking thread.
    pub(super) fn get(&self, url: &str, parses: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>, String> {
        let cache_file = self
            .cache_dir
            .as_ref()
            .map(|dir| dir.join(cache_filename(url)));
        let cached = cache_file.as_deref().and_then(Cached::read);
        if let Some(cached) = &cached
            && cached.is_fresh()
        {
            return Ok(cached.content.clone());
        }
        match self.download(url, cache_file.as_deref(), cached.as_ref(), parses) {
            Ok(content) => Ok(content),
            Err(failure) => match cached {
                Some(cached) if !failure.client_error => {
                    let days =
                        epoch_secs(SystemTime::now()).saturating_sub(cached.downloaded) / 86_400;
                    warn_user!(
                        "`{url}` could not be refreshed, using the cached copy from {days} days ago: {}",
                        failure.message
                    );
                    cached.touch();
                    Ok(cached.content)
                }
                _ => Err(failure.message),
            },
        }
    }

    /// Sends the request, revalidating `cached` when there is one.
    fn download(
        &self,
        url: &str,
        cache_file: Option<&Path>,
        cached: Option<&Cached>,
        parses: impl Fn(&[u8]) -> bool,
    ) -> Result<Vec<u8>, Failure> {
        let cached_etag = cached.and_then(|cached| cached.etag.as_deref());
        let mirrored = self.mirror.as_ref().and_then(|mirror| mirror.rewrite(url));
        if let Some(mirrored) = &mirrored {
            debug!("Fetching `{url}` from mirror `{mirrored}`");
        }
        let fetch_url = mirrored.as_deref().unwrap_or(url);
        let client = match &self.client {
            Some(client) => client.clone(),
            None => REQWEST_CLIENT.clone(),
        };
        let mut last_error = String::new();
        let mut client_error = false;
        for _ in 0..ATTEMPTS {
            let request = fetch(&client, fetch_url, cached_etag);
            let response = match self.runtime.block_on(request) {
                Ok(response) => response,
                Err(err) => {
                    last_error = format!("encountered error during download: {err}");
                    continue;
                }
            };
            if let Some(cached) = cached
                && (response.status == NOT_MODIFIED
                    || ((200..300).contains(&response.status) && is_cache_hit(cached, &response)))
            {
                cached.touch();
                return Ok(cached.content.clone());
            }
            if !(200..300).contains(&response.status) {
                last_error = format!(
                    "got response with status={}, retries exhausted",
                    response.status
                );
                // A `4xx` is the server's final answer. Retrying could end on a network error
                // or `5xx` instead, which would let a stale cached copy hide a deleted schema.
                if (400..500).contains(&response.status) {
                    client_error = true;
                    break;
                }
                continue;
            }
            if !parses(&response.body) {
                last_error = "downloaded content could not be parsed, retries exhausted".into();
                continue;
            }
            if let Some(path) = cache_file
                && let Err(err) = store(path, &response)
            {
                debug!("Failed to cache `{url}` at `{}`: {err}", path.display());
            }
            return Ok(response.body);
        }
        let message = match &mirrored {
            Some(mirrored) => {
                format!("failed to download `{url}` from mirror `{mirrored}`: {last_error}")
            }
            None => format!("failed to download `{url}`: {last_error}"),
        };
        Err(Failure {
            message,
            client_error,
        })
    }
}

async fn fetch(
    client: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
) -> reqwest::Result<Response> {
    let mut request = client.get(url);
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    let status = response.status().as_u16();
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let last_modified = header(LAST_MODIFIED);
    let etag = header(ETAG);
    let body = response.bytes().await?.to_vec();
    Ok(Response {
        status,
        last_modified,
        etag,
        body,
    })
}

/// The `ETag` of a cache file is saved next to it.
fn etag_path(cache_file: &Path) -> PathBuf {
    let mut name = cache_file.as_os_str().to_owned();
    name.push(".etag");
    PathBuf::from(name)
}

/// The download time of a cache file, in seconds since the epoch, is saved next to it.
fn downloaded_path(cache_file: &Path) -> PathBuf {
    let mut name = cache_file.as_os_str().to_owned();
    name.push(".downloaded");
    PathBuf::from(name)
}

fn epoch_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn store(path: &Path, response: &Response) -> std::io::Result<()> {
    write_atomic(path, &response.body)?;
    let downloaded = epoch_secs(SystemTime::now()).to_string();
    write_atomic(&downloaded_path(path), downloaded.as_bytes())?;
    match &response.etag {
        Some(etag) => write_atomic(&etag_path(path), etag.as_bytes()),
        None => match fs_err::remove_file(etag_path(path)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        },
    }
}

/// sha256 of the URL, plus the extension of its last path segment when it has one.
pub(super) fn cache_filename(url: &str) -> String {
    let hash = hex::encode(digest(&SHA256, url.as_bytes()));
    let last_part = url.rsplit('/').next().unwrap_or(url);
    match last_part.rsplit_once('.') {
        Some((_, extension)) => format!("{hash}.{extension}"),
        None => hash,
    }
}

/// Whether a `2xx` response leaves the cached copy current.
fn is_cache_hit(cached: &Cached, response: &Response) -> bool {
    if let Some(remote) = response.last_modified.as_deref().and_then(parse_http_date) {
        return cached.downloaded >= remote;
    }
    match &response.etag {
        Some(etag) => cached.etag.as_deref() == Some(etag.as_str()),
        None => true,
    }
}

/// Writes through a temporary file in the cache directory, so concurrent hooks never read
/// a partial file.
fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs_err::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut file, content)?;
    file.persist(path).map_err(|err| err.error)?;
    Ok(())
}

/// Parses `Sun, 01 Jan 2000 00:00:01 GMT` into seconds since the epoch, read as UTC.
pub(super) fn parse_http_date(value: &str) -> Option<u64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?;
    let day: u64 = parts.next()?.parse().ok()?;
    let month_name = parts.next()?;
    let month = MONTHS.iter().position(|name| *name == month_name)? as u64 + 1;
    let year: u64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 61 || year < 1970 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_filename_keeps_extension() {
        let name = cache_filename("https://example.com/schema1.json");
        assert_eq!(
            name.rsplit_once('.').map(|(_, ext)| ext),
            Some("json"),
            "{name}"
        );
        assert_eq!(name.len(), 64 + 5);
        assert_eq!(cache_filename("https://example.com/schema1").len(), 64);
        assert_ne!(
            cache_filename("https://a.example/s.json"),
            cache_filename("https://b.example/s.json")
        );
    }

    #[test]
    fn mirror_rewrites_schemastore_urls() {
        let mirror = Mirror::parse("https://mirror.corp/schemastore").unwrap();
        for host in [
            "https://json.schemastore.org",
            "https://www.schemastore.org",
            "http://json.schemastore.org",
            "http://www.schemastore.org",
        ] {
            assert_eq!(
                mirror
                    .rewrite(&format!("{host}/github-workflow.json"))
                    .as_deref(),
                Some("https://mirror.corp/schemastore/github-workflow.json"),
                "{host}"
            );
        }
        assert_eq!(
            mirror
                .rewrite("https://json.schemastore.org/dir/schema.json?v=2#/defs")
                .as_deref(),
            Some("https://mirror.corp/schemastore/dir/schema.json?v=2#/defs")
        );
    }

    #[test]
    fn mirror_joins_with_one_slash() {
        for base in [
            "https://mirror.corp/schemastore",
            "https://mirror.corp/schemastore/",
            "https://mirror.corp/schemastore//",
        ] {
            let mirror = Mirror::parse(base).unwrap();
            assert_eq!(
                mirror
                    .rewrite("https://json.schemastore.org/a.json")
                    .as_deref(),
                Some("https://mirror.corp/schemastore/a.json"),
                "{base}"
            );
        }
        let mirror = Mirror::parse("http://localhost:8080").unwrap();
        assert_eq!(
            mirror
                .rewrite("https://www.schemastore.org/a.json")
                .as_deref(),
            Some("http://localhost:8080/a.json")
        );
    }

    #[test]
    fn mirror_leaves_other_hosts() {
        let mirror = Mirror::parse("https://mirror.corp/schemastore").unwrap();
        for url in [
            "https://raw.githubusercontent.com/org/repo/main/schema.json",
            "https://schemastore.org/a.json",
            "https://json.schemastore.org.evil.example/a.json",
            "https://example.com/https://json.schemastore.org/a.json",
        ] {
            assert_eq!(mirror.rewrite(url), None, "{url}");
        }
    }

    #[test]
    fn mirror_must_be_absolute_http_url() {
        for value in [
            "",
            "mirror.corp/schemastore",
            "/srv/schemastore",
            "ftp://mirror.corp/schemastore",
            "file:///srv/schemastore",
            "https://",
        ] {
            let err = Mirror::parse(value).unwrap_err();
            assert!(err.contains("PREK_JSONSCHEMA_MIRROR"), "{value}: {err}");
        }
    }

    #[test]
    fn http_dates_are_utc() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(
            parse_http_date("Sun, 01 Jan 2000 00:00:01 GMT"),
            Some(946_684_801)
        );
        assert_eq!(
            parse_http_date("Tue, 29 Feb 2000 12:00:00 GMT"),
            Some(951_825_600)
        );
    }

    #[test]
    fn malformed_http_dates_are_ignored() {
        for value in [
            "",
            "Jan 2000",
            "Sun, 01 Foo 2000 00:00:01 GMT",
            "Sun, 01 Jan 99999999999999999999 00:00:01 GMT",
            "Sun, 32 Jan 2000 00:00:01 GMT",
        ] {
            assert_eq!(parse_http_date(value), None, "{value}");
        }
    }
}
