use std::cmp::Ordering;
use std::fmt::Write;

use bytes::{Buf, Bytes};
use either::Either;
use http::header::{self, HeaderValue};
use http::uri::{self, Uri};
use http::{Request, StatusCode};
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http2::SendRequest;
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOST: &'static str = "api.twitter.com";

const AUTHORITY: HeaderValue = HeaderValue::from_static(DEFAULT_HOST);

type Body =
    Either<bytes::buf::Reader<Bytes>, flate2::bufread::GzDecoder<bytes::buf::Reader<Bytes>>>;

#[derive(Debug, Deserialize, Serialize)]
pub struct Tweet {
    pub id: u64,
    pub user: User,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    pub id: u64,
}

#[derive(Debug)]
pub struct ListsStatuses {
    list_id: u64,
    since_id: Option<u64>,
    count: usize,
    include_entities: bool,
    uri: Uri,
}

impl Tweet {
    pub fn cmp_rev_id(&self, other: &Self) -> Ordering {
        other.id.cmp(&self.id)
    }
}

macro_rules! define_setters {
    ($($name:ident: $ty:ty;)*) => {$(
        pub fn $name(&mut self, $name: $ty) -> &mut Self {
            if self.$name != $name {
                self.$name = $name;
                self.format_uri();
            }
            self
        }
    )*};
}

impl ListsStatuses {
    pub fn new(list_id: u64) -> Self {
        let mut ret = Self {
            list_id,
            since_id: None,
            count: 20,
            include_entities: false,
            uri: Uri::default(),
        };
        ret.format_uri();
        ret
    }

    define_setters! {
        since_id: Option<u64>;
        count: usize;
    }

    fn format_uri(&mut self) {
        let mut uri = format!(
            "/1.1/lists/statuses.json?count={}&include_entities={}&list_id={}",
            self.count, self.include_entities, self.list_id
        );
        if let Some(since_id) = self.since_id {
            write!(uri, "&since_id={}", since_id).unwrap();
        }

        let mut parts = uri::Parts::default();
        parts.path_and_query = Some(uri.try_into().unwrap());
        self.uri = Uri::from_parts(parts).unwrap();
    }

    #[tracing::instrument(level = "trace", skip(bearer, request_sender))]
    pub async fn send(
        &self,
        bearer: HeaderValue,
        request_sender: &mut SendRequest<Empty<Bytes>>,
    ) -> anyhow::Result<Body> {
        const GZIP: HeaderValue = HeaderValue::from_static("gzip");

        let request = Request::get(self.uri.clone())
            .header(header::HOST, AUTHORITY)
            .header(header::ACCEPT_ENCODING, GZIP)
            .header(header::AUTHORIZATION, bearer)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let response = request_sender.send_request(request).await?;

        if response.status() != StatusCode::OK {
            anyhow::bail!("Bad status: {}", response.status());
        }

        let br = response
            .headers()
            .get(header::CONTENT_ENCODING)
            .map_or(false, |v| v == GZIP);

        let body = response.into_body().collect().await?.to_bytes();
        let body = if br {
            Either::Right(flate2::bufread::GzDecoder::new(body.reader()))
        } else {
            tracing::debug!("Response is in `identity` encoding");
            Either::Left(body.reader())
        };

        Ok(body)
    }
}
