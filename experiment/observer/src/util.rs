mod tokiort;

use std::cmp::Ordering;
use std::fmt;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use futures_util::FutureExt;
use http_body_util::Empty;
use hyper::header::HeaderValue;
use serde::de;

use self::tokiort::TokioExecutor;

pub const HTTPS_DEFAULT_PORT: u16 = 443;
pub const USER_AGENT: HeaderValue = HeaderValue::from_static(concat!(
    "leaky-snowflake-observer/",
    env!("CARGO_PKG_VERSION")
));

const TWEPOCH: u64 = 1288834974657;

const CLOCK_TOO_EARLY: &str = r#"\
Greetings from 2023! Unfortunately, your system clock is living an era before the birth of Twitter, \
a social media service of our era, which this program is targeted at"#;
const CLOCK_TOO_LATE: &str = r#"\
Greetings from 2023 CE! Unfortunately, your system clock has exceeded the capacity of the clock of Twitter, \
a social media service of our era, which this program is targeted at"#;

/// A `DeserializeSeed` implementation that reuses the given vector to deserialize a sequence.
pub struct DeserializeIntoVec<'a, T>(pub &'a mut Vec<T>);

/// Polyfill for the unstable `<[T]>::is_sorted_by` method.
///
/// <https://github.com/rust-lang/rust/issues/53485>
pub trait SliceIsSortedExt<T> {
    fn is_sorted_by<F: FnMut(&T, &T) -> Option<Ordering>>(&self, f: F) -> bool;
}

impl<T> SliceIsSortedExt<T> for [T] {
    fn is_sorted_by<F: FnMut(&T, &T) -> Option<Ordering>>(&self, mut f: F) -> bool {
        self.windows(2).all(|w| {
            let (u, t) = (&w[1], &w[0]);
            f(t, u).map_or(false, Ordering::is_le)
        })
    }
}

impl<'de, 'a, T> de::DeserializeSeed<'de> for DeserializeIntoVec<'a, T>
where
    T: de::Deserialize<'de>,
{
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        struct Visitor<'a, T: 'a>(&'a mut Vec<T>);

        impl<'de, 'a, T: de::Deserialize<'de>> de::Visitor<'de> for Visitor<'a, T> {
            type Value = ();

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a sequence")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
                self.0.clear();

                if let Some(hint) = seq.size_hint() {
                    self.0.reserve(hint);
                }

                while let Some(t) = seq.next_element()? {
                    self.0.push(t);
                }

                Ok(())
            }
        }

        deserializer.deserialize_seq(Visitor(self.0))
    }
}

/// Returns the first element of the set difference `x - y`,
/// assuming that `x` and `y` is sorted by the ordering given by `comparator`.
pub fn first_diff_sorted_by<T>(
    x: impl IntoIterator<Item = T>,
    y: impl IntoIterator<Item = T>,
    mut comparator: impl FnMut(&T, &T) -> Ordering,
) -> Option<T> {
    let mut y = y.into_iter();
    'outer: for t in x {
        for u in &mut y {
            match comparator(&t, &u) {
                Ordering::Greater => {}
                Ordering::Equal => continue 'outer,
                Ordering::Less => return Some(t),
            }
        }
        return Some(t);
    }

    None
}

pub async fn http2_connect(
    host: &str,
    port: u16,
) -> anyhow::Result<hyper::client::conn::http2::SendRequest<Empty<Bytes>>> {
    let stream = tokio::net::TcpStream::connect((host, port)).await?;
    let tls_connector: tokio_native_tls::TlsConnector =
        tokio_native_tls::native_tls::TlsConnector::new()
            .expect("Error initializing TLS connector")
            .into();
    let stream = tls_connector.connect(host, stream).await?;
    let (ret, conn) = hyper::client::conn::http2::Builder::new()
        .executor(TokioExecutor)
        .handshake(stream)
        .await?;

    tokio::spawn(conn.map(|result| {
        if let Err(e) = result {
            tracing::error!("Error in HTTP connection: {}", e);
        }
    }));

    Ok(ret)
}

pub fn time_to_unix(time: SystemTime) -> Duration {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .expect(CLOCK_TOO_EARLY)
}

pub fn time_to_unix_ms(time: SystemTime) -> u64 {
    unix_to_ms(time_to_unix(time))
}

pub fn unix_to_ms(unix: Duration) -> u64 {
    unix.as_millis().try_into().expect(CLOCK_TOO_LATE)
}

pub fn unix_ms_to_sf(unix_ms: u64) -> u64 {
    unix_ms
        .checked_sub(TWEPOCH)
        .expect(CLOCK_TOO_EARLY)
        .checked_shl(22)
        .expect(CLOCK_TOO_LATE)
}
