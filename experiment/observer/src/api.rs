use std::cmp::Ordering;

use bytes::{Buf, Bytes};
use either::Either;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http2::SendRequest;
use hyper::header::{self, HeaderValue};
use hyper::{Request, StatusCode};
use serde::{Deserialize, Serialize};

pub const HOST: &'static str = "api.twitter.com";

const AUTHORITY: HeaderValue = HeaderValue::from_static(HOST);
const ENDPOINT: &'static str = "https://api.twitter.com/1.1/lists/statuses.json";
const ENDPOINT_PATH: &'static str = "/1.1/lists/statuses.json";

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

#[derive(Debug, oauth::Request)]
pub struct ListsStatuses {
    list_id: u64,
    since_id: Option<u64>,
    count: usize,
    include_entities: bool,
}

impl Tweet {
    pub fn cmp_rev_id(&self, other: &Self) -> Ordering {
        other.id.cmp(&self.id)
    }
}

impl ListsStatuses {
    pub fn new(list_id: u64) -> Self {
        Self {
            list_id,
            since_id: None,
            count: 20,
            include_entities: false,
        }
    }

    pub fn since_id(&mut self, since_id: Option<u64>) -> &mut Self {
        self.since_id = since_id;
        self
    }

    pub fn count(&mut self, count: usize) -> &mut Self {
        self.count = count;
        self
    }

    #[tracing::instrument(level = "trace", skip(request_sender))]
    pub async fn send(
        &self,
        token: &oauth::Token,
        request_sender: &mut SendRequest<Empty<Bytes>>,
    ) -> anyhow::Result<Body> {
        const GZIP: HeaderValue = HeaderValue::from_static("gzip");

        let authorization = oauth::get(ENDPOINT, self, token, oauth::HMAC_SHA1);
        let uri = oauth::to_query(ENDPOINT_PATH.to_owned(), self);
        let request = Request::get(uri)
            .header(header::HOST, AUTHORITY)
            .header(header::ACCEPT_ENCODING, GZIP)
            .header(header::AUTHORIZATION, authorization)
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
