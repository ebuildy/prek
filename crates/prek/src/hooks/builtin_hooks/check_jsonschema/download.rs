//! Remote schema downloads with an on-disk cache, ported from upstream `cachedownloader.py`.
//!
//! Every lookup sends a GET. The cached copy is used when its mtime is at least the
//! response's `Last-Modified`. Without that header, prek compares the response's `ETag`
//! with the one saved next to the cache file and sends it as `If-None-Match`, so a `304`
//! also reuses the cache (upstream issue #668). With neither header, an existing cache file
//! always wins, like upstream. A fresh body must parse before it is cached; a body that
//! does not parse is retried like a failed request.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use aws_lc_rs::digest::{SHA256, digest};
use reqwest::header::{ETAG, IF_NONE_MATCH, LAST_MODIFIED};
use tokio::runtime::Handle;
use tracing::debug;

use crate::http::REQWEST_CLIENT;

/// One request plus two retries, like upstream.
const ATTEMPTS: usize = 3;

pub(super) struct Downloader {
    /// `None` means prek's shared client. It is only touched when a download happens,
    /// because creating it loads TLS certificates, which would slow down local-only runs.
    client: Option<reqwest::Client>,
    /// `None` when `--no-cache` is set.
    cache_dir: Option<PathBuf>,
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

impl Downloader {
    pub(super) fn new(
        client: Option<reqwest::Client>,
        cache_dir: Option<PathBuf>,
        runtime: Handle,
    ) -> Self {
        Self {
            client,
            cache_dir,
            runtime,
        }
    }

    /// Downloads `url`, or reads it from the cache. `parses` decides whether a fresh body is
    /// usable. This blocks on the runtime, so call it from a blocking thread.
    pub(super) fn get(&self, url: &str, parses: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>, String> {
        let cache_file = self
            .cache_dir
            .as_ref()
            .map(|dir| dir.join(cache_filename(url)));
        let cached_etag = cache_file
            .as_ref()
            .and_then(|path| fs_err::read_to_string(etag_path(path)).ok());
        let client = match &self.client {
            Some(client) => client.clone(),
            None => REQWEST_CLIENT.clone(),
        };
        let mut last_error = String::new();
        for _ in 0..ATTEMPTS {
            let request = fetch(&client, url, cached_etag.as_deref());
            let response = match self.runtime.block_on(request) {
                Ok(response) => response,
                Err(err) => {
                    last_error = format!("encountered error during download: {err}");
                    continue;
                }
            };
            if response.status == NOT_MODIFIED
                && let Some(path) = &cache_file
                && let Ok(content) = fs_err::read(path)
            {
                return Ok(content);
            }
            if !(200..300).contains(&response.status) {
                last_error = format!(
                    "got response with status={}, retries exhausted",
                    response.status
                );
                continue;
            }
            if let Some(path) = &cache_file
                && is_cache_hit(path, &response, cached_etag.as_deref())
                && let Ok(content) = fs_err::read(path)
            {
                return Ok(content);
            }
            if !parses(&response.body) {
                last_error = "downloaded content could not be parsed, retries exhausted".into();
                continue;
            }
            if let Some(path) = &cache_file
                && let Err(err) = store(path, &response)
            {
                debug!("Failed to cache `{url}` at `{}`: {err}", path.display());
            }
            return Ok(response.body);
        }
        Err(format!("failed to download `{url}`: {last_error}"))
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

fn store(path: &Path, response: &Response) -> std::io::Result<()> {
    write_atomic(path, &response.body)?;
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

fn is_cache_hit(path: &Path, response: &Response, cached_etag: Option<&str>) -> bool {
    let Ok(modified) = fs_err::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    if let Some(remote) = response.last_modified.as_deref().and_then(parse_http_date) {
        let local = modified
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        return local >= remote;
    }
    match &response.etag {
        Some(etag) => cached_etag == Some(etag.as_str()),
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
