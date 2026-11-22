//! Client files gzipped once. A content's gzip is made at the best level on the first request for
//! it that takes gzip, and later requests, from any project the server serves, send those stored
//! bytes. The store is keyed by the content's SHA-256, not by the file's name, so a file rewritten
//! under its name is gzipped anew. What is kept is the most recently asked-for gzips up to
//! [`KEPT`] bytes; past it the least recently asked for is dropped, and gzipped again if it is
//! asked for again.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use symphony_board::lock::Recovered as _;

use axum::body::Bytes;
use axum::http::{HeaderMap, header};
use flate2::Compression;
use flate2::write::GzEncoder;
use tokio::sync::OnceCell;

/// The most bytes the server keeps gzipped: about eight builds of every page's files (one build is
/// some 2 MB gzipped), room for the build served and the older ones open pages still load from,
/// and no more however often the server is rebuilt.
const KEPT: usize = 16 << 20;

static STORED: LazyLock<Store> = LazyLock::new(|| Store::new(KEPT));

/// Content gzips by the SHA-256 the file's ETag names, at most `cap` bytes of them.
pub(super) struct Store {
    cap: usize,
    kept: Mutex<Kept>,
    /// How many times each content was compressed, for the tests that a later request does no
    /// compression work.
    #[cfg(test)]
    runs: Mutex<HashMap<String, usize>>,
}

#[derive(Default)]
struct Kept {
    entries: HashMap<String, Entry>,
    /// What the made entries count, added up.
    bytes: usize,
    /// Ticks on each request, so the entry with the lowest `used` is the least recently asked for.
    clock: u64,
}

struct Entry {
    /// The gzip, or `None` for a content gzip does not make smaller.
    gzip: Arc<OnceCell<Option<Bytes>>>,
    used: u64,
    /// The gzip's length and its key's, once made; 0 while it is being made, when it is not
    /// dropped.
    size: usize,
}

/// `bytes`, whose SHA-256 is `hash`, gzipped: made by the first call for that content, which
/// concurrent calls wait for and later calls share. `None` when gzip would not make it smaller, or
/// when the encoder failed, which the next call tries again.
pub(super) async fn gzipped(hash: &str, bytes: &Bytes) -> Option<Bytes> {
    STORED.gzipped(hash, bytes).await
}

#[cfg(test)]
pub(super) fn runs(hash: &str) -> usize {
    STORED.runs(hash)
}

impl Store {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            cap,
            kept: Mutex::default(),
            #[cfg(test)]
            runs: Mutex::default(),
        }
    }

    pub(super) async fn gzipped(&self, hash: &str, bytes: &Bytes) -> Option<Bytes> {
        let gzip = {
            let mut kept = self.lock();
            kept.clock += 1;
            let used = kept.clock;
            let entry = kept
                .entries
                .entry(hash.to_owned())
                .or_insert_with(|| Entry {
                    gzip: Arc::default(),
                    used,
                    size: 0,
                });
            entry.used = used;
            entry.gzip.clone()
        };
        let made = gzip
            .get_or_try_init(|| async {
                #[cfg(test)]
                {
                    *self
                        .runs
                        .lock()
                        .recover()
                        .entry(hash.to_owned())
                        .or_default() += 1;
                }
                // The best level holds a thread a while on a large module: a blocking one, not a worker.
                let raw = bytes.clone();
                let zipped = crate::spawn::blocking(move || compress(&raw))
                    .await
                    .map_err(io::Error::other)??;
                let zipped = (zipped.len() < bytes.len()).then(|| Bytes::from(zipped));
                self.made(hash, &gzip, zipped.as_ref().map_or(0, Bytes::len));
                Ok::<_, io::Error>(zipped)
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

    /// Counts `hash`'s new gzip of `length` bytes, then drops the least recently asked-for made
    /// entries until what is kept fits the cap.
    fn made(&self, hash: &str, gzip: &Arc<OnceCell<Option<Bytes>>>, length: usize) {
        let mut guard = self.lock();
        let kept = &mut *guard;
        let Some(entry) = kept
            .entries
            .get_mut(hash)
            .filter(|entry| Arc::ptr_eq(&entry.gzip, gzip))
        else {
            return;
        };
        entry.size = length + hash.len();
        kept.bytes += entry.size;
        while kept.bytes > self.cap {
            let Some(oldest) = kept
                .entries
                .iter()
                .filter(|(_, entry)| entry.size > 0)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(hash, _)| hash.clone())
            else {
                break;
            };
            if let Some(dropped) = kept.entries.remove(&oldest) {
                kept.bytes -= dropped.size;
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().recover()
    }

    /// The bytes kept, which the cap bounds.
    #[cfg(test)]
    pub(super) fn kept(&self) -> usize {
        self.lock().bytes
    }

    #[cfg(test)]
    pub(super) fn runs(&self, hash: &str) -> usize {
        self.runs.lock().recover().get(hash).copied().unwrap_or(0)
    }
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

/// `bytes` gzipped at the best level.
fn compress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes)?;
    encoder.finish()
}
