This directory contains an experimental code to detect timeline leaks prophesied in the [main article](../README.md).

See the [Japanese version] of the article for details on the experiment design and the results (initially, I was planning on writing it in English as well, but I no longer have the enthusiasm for Twitter API enough to motivate myself to write up, after all the recent events on the platform). TL;DR: we've caught an instance of timeline leak! But our approach (assuming $k = 1 \mathrm{s}$ would've missed that instance too...

[Japanese version]: <https://zenn.dev/tesaguri/articles/leaky-snowflake-experiment>

## Requirements

- A set of of User Access Tokens of Twitter API with the [Elevated access] to API v1.1 endpoints
- Rust language toolchain
- Ruby language interpreter (optional. Used by a helper script)
- [`twurl`] tool with the API tokens set up (optional. This simplifies the management of API credentials)

[Elevated access]: <https://developer.twitter.com/en/docs/twitter-api/getting-started/about-twitter-api>
[`twurl`]: <https://github.com/twitter/twurl>

## Usage

To run the experiment, execute the following command:

```shell
RUST_LOG='leaky_snowflake_observer=info' cargo run --release -- -k 2000 [LIST_ID]
```

where `[LIST_ID]` is the ID of a Twitter List to observe. The API credentials is required to be authorized the access to the List.

This will poll the List timeline using the approach proposed in the main article, with the assumption of $k = 2000 \mathrm{ms}$ (using a higher value just to be sure), and when detects a timeline leaks, reports the contents of the timelines fetched in the latest and previous requests, along with other data like `latest_id` of that time.

## License

See [`COPYING.md`](../COPYING.md) for the copyright notice and license of the experimental code.
