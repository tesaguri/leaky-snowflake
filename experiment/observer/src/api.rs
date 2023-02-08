macro_rules! def_timelines {
    ($(
        $path:literal;
        $(#[$attr:meta])*
        $vis:vis struct $Name:ident {
            $($ctor_arg:ident: $C:ty,)*
            @since_id $since_id:ident: Option<u64>
            $(, $param:ident: $P:ty = $param_default:expr)* $(,)?
        }
    )*) => {$(
        $(#[$attr])*
        $vis struct $Name {
            $($ctor_arg: $C,)*
            $since_id: Option<u64>,
            $($param: $P,)*
        }

        impl $Name {
            pub fn new($($ctor_arg: $C),*) -> Self {
                Self {
                    $($ctor_arg,)*
                    $since_id: None,
                    $($param: $param_default,)*
                }
            }
        }

        impl $crate::api::TimelineRequest for $Name {
            fn set_since_id(&mut self, since_id: Option<u64>) {
                self.$since_id = since_id;
            }

            fn fetch<D>(
                &self,
                seed: D,
                token: &$crate::api::Token,
                request_sender: &mut hyper::client::conn::http2::SendRequest<
                    http_body_util::Empty<bytes::Bytes>,
                >,
            ) -> $crate::api::ResponseFuture<D>
            where
                D: for<'de> serde::de::DeserializeSeed<'de>,
            {
                const ENDPOINT: &str = concat!("https://api.twitter.com", $path);
                const PATH: &str = $path;

                let response = Box::pin($crate::api::send_request(
                    self,
                    ENDPOINT,
                    PATH,
                    token,
                    request_sender,
                ));
                let inner = $crate::api::response::Inner::Response { response };
                $crate::api::ResponseFuture { inner, seed: Some(seed) }
            }
        }
    )*};
}

pub mod lists;

mod response;

pub use self::response::ResponseFuture;

use std::cmp::Ordering;
use std::future::Future;

use bytes::Bytes;
use http_body_util::Empty;
use hyper::client::conn::http2::SendRequest;
use hyper::header::{self, HeaderValue};
use hyper::{Request, Response, Uri};
use serde::{de::DeserializeSeed, Deserialize, Serialize};

use crate::util;

pub const HOST: &'static str = "api.twitter.com";

const AUTHORITY: HeaderValue = HeaderValue::from_static(HOST);
const GZIP: HeaderValue = HeaderValue::from_static("gzip");

pub enum Token {
    UserContext(oauth::Token),
    AppOnly(HeaderValue),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Tweet {
    pub id: u64,
    pub user: User,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    pub id: u64,
}

pub trait TimelineRequest {
    fn set_since_id(&mut self, since_id: Option<u64>);
    fn fetch<D>(
        &self,
        seed: D,
        token: &Token,
        request_sender: &mut SendRequest<Empty<Bytes>>,
    ) -> ResponseFuture<D>
    where
        D: for<'de> DeserializeSeed<'de>;
}

impl Token {
    pub fn from_bearer(bearer: &str) -> Option<Self> {
        HeaderValue::try_from(format!("Bearer {}", bearer))
            .ok()
            .map(Token::AppOnly)
    }
}

impl From<oauth::Token> for Token {
    fn from(token: oauth::Token) -> Self {
        Token::UserContext(token)
    }
}

impl Tweet {
    pub fn cmp_rev_id(&self, other: &Self) -> Ordering {
        other.id.cmp(&self.id)
    }
}

fn send_request<R>(
    request: &R,
    endpoint: &str,
    path: &str,
    token: &Token,
    request_sender: &mut SendRequest<Empty<Bytes>>,
) -> impl Future<Output = Result<hyper::Response<hyper::body::Incoming>, hyper::Error>>
where
    R: oauth::Request,
{
    tracing::trace!(endpoint, "ep");
    let authorization = match *token {
        Token::UserContext(ref token) => oauth::get(endpoint, request, token, oauth::HMAC_SHA1)
            .try_into()
            .unwrap(),
        Token::AppOnly(ref authorization) => authorization.clone(),
    };
    let uri = Uri::try_from(oauth::to_query(path.to_owned(), request)).unwrap();

    fn inner(
        uri: Uri,
        authorization: HeaderValue,
        request_sender: &mut SendRequest<Empty<Bytes>>,
    ) -> impl Future<Output = hyper::Result<Response<hyper::body::Incoming>>> {
        let request = Request::get(uri)
            .header(header::HOST, AUTHORITY)
            .header(header::ACCEPT_ENCODING, GZIP)
            .header(header::AUTHORIZATION, authorization)
            .header(header::USER_AGENT, util::USER_AGENT)
            .body(Empty::<Bytes>::new())
            .unwrap();
        request_sender.send_request(request)
    }

    inner(uri, authorization, request_sender)
}
