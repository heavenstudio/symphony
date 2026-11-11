//! Client files gzipped once. A content's gzip is made at the best level on the first request for
//! it that takes gzip, and every later request, from any project the server serves, sends those
//! stored bytes: the build's files are content-hashed, so a name's bytes never change under it.
//! What is kept is what was asked for, at most the client's files gzipped.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use axum::body::Bytes;
use axum::http::{HeaderMap, header};
use flate2::Compression;
use flate2::write::GzEncoder;
use tokio::sync::OnceCell;

/// Each content's gzip, by the SHA-256 the file's ETag names, kept for the process's life. `None`
/// is a content gzip does not make smaller.
type Stored = Mutex<HashMap<String, Arc<OnceCell<Option<Bytes>>>>>;

static GZIPPED: LazyLock<Stored> = LazyLock::new(Stored::default);

/// How many times each content was compressed, for the tests that a later request does no
/// compression work.
#[cfg(test)]
static RUNS: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Mutex::default);

#[cfg(test)]
pub(super) fn runs(hash: &str) -> usize {
    RUNS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(hash)
        .copied()
        .unwrap_or(0)
}

/// Whether `headers` take gzip over the file as it is, as the router's compression layer reads
/// them: `gzip` or `x-gzip` with a nonzero quality and none higher for `identity`. An entry whose
/// quality cannot be read, and `*`, count for nothing there, so here too.
pub(super) fn takes_gzip(headers: &HeaderMap) -> bool {
    let (mut gzip, mut identity) = (None::<u16>, 0_u16);
    for entry in headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
    {
        let mut parts = entry.splitn(2, ';');
        let coding = parts.next().unwrap_or_default().trim();
        let Some(weight) = parts.next().map_or(Some(1000), |q| quality(q.trim())) else {
            continue;
        };
        if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
            gzip = gzip.max(Some(weight));
        } else if coding.eq_ignore_ascii_case("identity") {
            identity = identity.max(weight);
        }
    }
    gzip.is_some_and(|gzip| gzip > 0 && gzip >= identity)
}

/// A `q=` parameter in thousandths: `0` or `1`, then up to three decimals, at most 1.
fn quality(parameter: &str) -> Option<u16> {
    let value = parameter
        .strip_prefix("q=")
        .or_else(|| parameter.strip_prefix("Q="))?;
    let (whole, decimals) = value.split_once('.').unwrap_or((value, ""));
    let whole = match whole {
        "0" => 0,
        "1" => 1000,
        _ => return None,
    };
    if decimals.len() > 3 || !decimals.bytes().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    let thousandths = decimals
        .bytes()
        .zip([100, 10, 1])
        .fold(whole, |sum, (digit, place)| {
            sum + u16::from(digit - b'0') * place
        });
    (thousandths <= 1000).then_some(thousandths)
}

/// `bytes`, whose SHA-256 is `hash`, gzipped: made by the first call for that content, which
/// concurrent calls wait for and later calls share. `None` when gzip would not make it smaller, or
/// when the encoder failed, which the next call tries again.
pub(super) async fn gzipped(hash: &str, bytes: &Bytes) -> Option<Bytes> {
    let stored = GZIPPED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(hash.to_owned())
        .or_default()
        .clone();
    let made = stored
        .get_or_try_init(|| async {
            #[cfg(test)]
            {
                *RUNS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .entry(hash.to_owned())
                    .or_default() += 1;
            }
            // The best level holds a thread a while on a page's wasm: a blocking one, not a worker.
            let raw = bytes.clone();
            let zipped = tokio::task::spawn_blocking(move || gzip(&raw))
                .await
                .map_err(io::Error::other)??;
            Ok::<_, io::Error>((zipped.len() < bytes.len()).then(|| Bytes::from(zipped)))
        })
        .await;
    match made {
        Ok(zipped) => zipped.clone(),
        Err(error) => {
            eprintln!("· a client file could not be gzipped, sent as it is: {error}");
            None
        }
    }
}

/// `bytes` gzipped at the best level.
fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}
