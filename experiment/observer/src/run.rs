use std::io::{stdout, Write};
use std::mem;
use std::ops::ControlFlow;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use bytes::Bytes;
use http_body_util::Empty;
use hyper::client::conn::http2::SendRequest;

use crate::api::{self, ListsStatuses, Tweet};
use crate::util::{self, SliceIsSortedExt};

const MAX_TIMELINE_LEN: usize = 200;
const INTERVAL: Duration = Duration::from_secs(1);

pub struct Args {
    pub list_id: u64,
    pub k_ms: u64,
    pub token: oauth::Token,
}

#[tracing::instrument(skip_all, fields(list_id = %args.list_id, k_ms = %args.k_ms))]
pub async fn run(args: Args) -> anyhow::Result<()> {
    let mut nth = 1;
    let mut request_sender = util::http2_connect(api::HOST, util::HTTPS_DEFAULT_PORT).await?;

    let (start_ms, mut interval) = {
        // Start the interval at exactly the beginning of a second of the clock
        // to make the output a bit cleaner and maybe to make the rate-limit
        // behavior and the experiment condition more consistent (e.g. speed
        // of the TL might be biased by subsecond values of the clock).
        let now_sys = SystemTime::now();
        let now = Instant::now();
        let now_unix = util::time_to_unix(now_sys);
        let now_subsec = now_unix - Duration::from_secs(now_unix.as_secs());
        let wait = Duration::from_secs(1) - now_subsec;
        let start = now + wait;
        let start_ms = util::unix_to_ms(now_unix + wait);
        (start_ms, tokio::time::interval_at(start.into(), INTERVAL))
    };

    let mut timeline = Vec::with_capacity(MAX_TIMELINE_LEN);
    let mut previous_state: Option<State> = None;
    loop {
        interval.tick().await;
        if let ControlFlow::Break(()) = request(
            &args,
            start_ms,
            nth,
            &mut previous_state,
            &mut timeline,
            &mut request_sender,
        )
        .await?
        {
            break;
        }
        nth += 1;
    }

    Ok(())
}

struct State {
    timeline: Vec<Tweet>,
    latest_id: u64,
    retrieved_ms: u64,
}

impl State {
    fn next_since_id(&self, k_ms: u64) -> u64 {
        let lower = (((self.latest_id >> 22) - k_ms) << 22) - 1;
        (util::unix_ms_to_sf(self.retrieved_ms - k_ms) - 1).clamp(lower, self.latest_id)
    }
}

#[tracing::instrument(skip_all, fields(nth, latest_id = previous_state.as_ref().map(|s| s.latest_id)))]
async fn request(
    &Args {
        list_id,
        k_ms,
        ref token,
    }: &Args,
    start_ms: u64,
    nth: u64,
    previous_state: &mut Option<State>,
    timeline: &mut Vec<Tweet>,
    request_sender: &mut SendRequest<Empty<Bytes>>,
) -> anyhow::Result<ControlFlow<()>> {
    let mut request = ListsStatuses::new(list_id);
    request.count(MAX_TIMELINE_LEN);
    if let Some(ref previous) = *previous_state {
        request.since_id(Some(previous.next_since_id(k_ms)));
    }

    let retrieved_ms = util::time_to_unix_ms(SystemTime::now());
    tracing::info!(?request, %retrieved_ms, "Initiating API request");
    let result = request.send(&token, request_sender).await;
    match result {
        Ok(body) => {
            timeline.clear();
            let mut deserializer = serde_json::Deserializer::from_reader(body);
            util::deserialize_into_vec(timeline, &mut deserializer)
                .and_then(|()| deserializer.end())
                .context("Twitter responded with unexpected format")?;
            tracing::info!(?timeline, "Request succeeded");
        }
        Err(cause) if cause.is::<hyper::Error>() => {
            tracing::error!(%cause, "Error in HTTP connection");
            // Attempt to reconnect
            *request_sender = util::http2_connect(api::HOST, util::HTTPS_DEFAULT_PORT).await?;
            return Ok(ControlFlow::Continue(()));
        }
        Err(cause) => {
            tracing::error!(%cause, "Error in API request");
            return Ok(ControlFlow::Continue(()));
        }
    };

    // Make sure the TL is sorted in reverse chronological order, just in case.
    // ... Well, reverse Snowflake ID order, I mean.
    #[allow(unstable_name_collisions)]
    if !timeline.is_sorted_by(|t, u| Some(t.cmp_rev_id(u))) {
        tracing::warn!("response is not sorted");
        timeline.sort_unstable_by(Tweet::cmp_rev_id);
    }

    if let Some(ref previous) = *previous_state {
        // Check if we've missed any Tweets in the previous request
        // whose ID is less than the largest one retrieved before.

        // First, slice the timelines so that they only contain IDs in the range
        // of `(since_id, latest_id]`. Note that the ordering of the timelines
        // and hence the slicing ranges are reversed ones.
        let since_id = previous.next_since_id(k_ms);
        let old = {
            let seek = since_id;
            let i = previous
                .timeline
                .binary_search_by(move |t| seek.cmp(&t.id))
                .unwrap_or_else(|i| i);
            &previous.timeline[..i]
        };
        let new = {
            let seek = previous.latest_id;
            let i = timeline
                .binary_search_by(move |t| seek.cmp(&t.id))
                .unwrap_or_else(|i| i);
            &timeline[i..]
        };

        tracing::debug!(
            ?new,
            ?old,
            "Comparing the overlapping part of the timelines..."
        );

        // Next, search for a "leaked" Tweet...
        if let Some(leaked) = util::first_diff_sorted_by(new, old, |t, u| t.cmp_rev_id(u)) {
            // Gotcha!
            tracing::info!(id = %leaked.id, "Observed a leaked status");

            let magic = if since_id == previous.latest_id {
                Some(true)
            } else {
                tracing::info!("Checking if the \"magic\" exists");
                request.since_id(Some(previous.latest_id));
                let result = request.send(&token, request_sender).await;
                match result {
                    Ok(body) => {
                        let mut timeline = Vec::<Tweet>::new();
                        let mut deserializer = serde_json::Deserializer::from_reader(body);
                        util::deserialize_into_vec(&mut timeline, &mut deserializer)
                            .and_then(|()| deserializer.end())
                            .context("Twitter responded with unexpected format")?;
                        tracing::info!(?timeline, "Request succeeded");
                        Some(timeline.iter().any(|t| t.id == leaked.id))
                    }
                    Err(cause) => {
                        // We're out of luck...
                        tracing::error!(?cause, "Error in API request");
                        None
                    }
                }
            };

            // Now, report the results and call it a day.
            #[derive(serde::Serialize)]
            struct Output<'a> {
                k_ms: u64,
                start_ms: u64,
                nth: u64,
                previous: Previous<'a>,
                latest: Latest<'a>,
                magic: Option<bool>,
            }
            #[derive(serde::Serialize)]
            struct Previous<'a> {
                retrieved_ms: u64,
                latest_id: u64,
                statuses: &'a [Tweet],
            }
            #[derive(serde::Serialize)]
            struct Latest<'a> {
                retrieved_ms: u64,
                statuses: &'a [Tweet],
            }
            let output = Output {
                k_ms,
                start_ms,
                nth,
                previous: Previous {
                    retrieved_ms: previous.retrieved_ms,
                    latest_id: previous.latest_id,
                    statuses: &previous.timeline,
                },
                latest: Latest {
                    retrieved_ms,
                    statuses: &timeline,
                },
                magic,
            };

            let mut stdout = stdout().lock();
            serde_json::to_writer(&mut stdout, &output)?;
            writeln!(stdout).unwrap();

            return Ok(ControlFlow::Break(()));
        }
    }

    if let Some(ref mut previous) = *previous_state {
        match *previous.timeline {
            [ref t, ..] if t.id > previous.latest_id => {
                previous.latest_id = t.id;
            }
            _ => {}
        }
        previous.retrieved_ms = retrieved_ms;
        mem::swap(&mut previous.timeline, timeline);
    } else if timeline.len() > 0 {
        *previous_state = Some(State {
            latest_id: timeline[0].id,
            timeline: mem::replace(timeline, Vec::with_capacity(MAX_TIMELINE_LEN)),
            retrieved_ms,
        });
    }

    Ok(ControlFlow::Continue(()))
}
