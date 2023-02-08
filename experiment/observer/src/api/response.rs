use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use bytes::Buf;
use http_body_util::{combinators::Collect, BodyExt};
use hyper::header;
use hyper::StatusCode;
use pin_project_lite::pin_project;
use serde::de::DeserializeSeed;

pin_project! {
    pub struct ResponseFuture<D> {
        #[pin]
        pub(super) inner: Inner,
        pub(super) seed: Option<D>,
    }
}

pin_project! {
    #[project = InnerProj]
    pub(super) enum Inner {
        Response {
            response: Pin<Box<dyn Future<Output = hyper::Result<hyper::Response<hyper::body::Incoming>>>>>,
        },
        Body {
            #[pin]
            body: Collect<hyper::body::Incoming>,
            gzip: bool,
        },
    }
}

struct Body<B> {
    buf: B,
    gzip: bool,
}

impl<D: for<'de> DeserializeSeed<'de>> Future for ResponseFuture<D> {
    type Output = anyhow::Result<<D as DeserializeSeed<'static>>::Value>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let Body { buf, gzip } = ready!(this.inner.poll(cx))?;
        let reader = buf.reader();

        let value = if gzip {
            let reader = flate2::bufread::GzDecoder::new(reader);
            let mut deserializer = serde_json::Deserializer::from_reader(reader);
            this.seed.take().unwrap().deserialize(&mut deserializer)?
        } else {
            let mut deserializer = serde_json::Deserializer::from_reader(reader);
            this.seed.take().unwrap().deserialize(&mut deserializer)?
        };

        return Poll::Ready(Ok(value));
    }
}

impl Inner {
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<anyhow::Result<Body<impl Buf>>> {
        loop {
            match self.as_mut().project() {
                InnerProj::Response { response } => {
                    let response = ready!(response.as_mut().poll(cx))?;

                    if response.status() != StatusCode::OK {
                        return Poll::Ready(Err(anyhow::anyhow!(
                            "Bad status: {}",
                            response.status()
                        )));
                    }

                    let gzip = response
                        .headers()
                        .get(header::CONTENT_ENCODING)
                        .map_or(false, |v| v == super::GZIP);
                    if !gzip {
                        tracing::debug!("Response is in `identity` encoding");
                    }

                    let body = response.into_body().collect();

                    self.set(Inner::Body { body, gzip });
                }
                InnerProj::Body { body, gzip } => {
                    let buf = ready!(body.poll(cx))?.aggregate();
                    return Poll::Ready(Ok(Body { buf, gzip: *gzip }));
                }
            }
        }
    }
}
