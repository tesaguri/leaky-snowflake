# Your Timelines Are Leaky: A Slipperiness of Twitter's Snowflake IDs and `since_id` ❄️

- <span lang="ja">[日本語版]（より詳細な説明~~および蛇足~~を含む）</span>

[日本語版]: <https://zenn.dev/tesaguri/articles/leaky-snowflake-theory>

## Abstract

In polling operations on timelines in Snowflake-ID order, it is a common optimization technique to skip statuses with IDs lower than or equal to the ID of the latest status using the `since_id` parameter of the API, on the assumption that the IDs are chronologically ordered. In fact however, the ordering of Snowflake IDs is not complete. Instead, they are $k$-sorted, where $k = 1 \mathrm{s}$ in Twitter's own implementation, meaning that the chronological ordering is guaranteed only when the timestamps of the IDs are at least $k = 1 \mathrm{s}$ apart from each other. This implies the possibility that newer statuses have IDs lower than the latest status observed in the last request, which the common technique would inadvertently skip. To capture such statuses, the client should adjust the `since_id` parameter value of the subsequent request so that the timestamp part of the ID is at least $k = 1 \mathrm{s}$ earlier than the time of the previous request.

## Background

Suppose your app is polling a timeline for new Tweets using Twitter API, that is, the app periodically fetches the timeline and processes new Tweets as they appear. This is a common operation typically seen in client apps that display a timeline to an end user... oh, no, third-party clients have been [banned][twitter-bans-clients] by Twitter. Well, another example is bot apps that respond to a timeline in real time... ugh, Twitter has declared a [battle against the bots]. Anyway, we are going to discuss this not-so-common-at-least-among-Twitter's-friends-anymore operation called polling... for fun, maybe? Though you will have to pay [~\$100/mo.][cheap-for-scam-yet-steep-for-volunteer] for this.

[twitter-bans-clients]: <https://www.engadget.com/twitter-new-developer-terms-ban-third-party-clients-211247096.html>
[battle against the bots]: <https://web.archive.org/web/20221105191921/https://apps.apple.com/us/app/twitter/id333903271>
[cheap-for-scam-yet-steep-for-volunteer]: <https://twitter.com/elonmusk/status/1621259936524300289>

A naïve approach to poll a timeline would be to fetch a constant number of latest Tweets in every request and filter out Tweets that have already been processed in previous requests on the client's side. This would cause same Tweets to be repeatedly returned by the API, wasting the network bandwidth and [Tweet caps].

[Tweet caps]: <https://developer.twitter.com/en/docs/twitter-api/tweet-caps>

The alternative way [recommended][working-with-timelines] by Twitter is to use the `since_id` parameter of the API. The parameter tells the API to only return Tweets with IDs higher than the specified value. Since Tweet IDs are said to be chronologically ordered, it is plausible that you can safely skip the redundant Tweets by setting the parameter to the ID of the latest Tweet the app has fetched before. At least, that's what the Twitter documentation claims.

[working-with-timelines]: <https://developer.twitter.com/en/docs/twitter-api/v1/tweets/timelines/guides/working-with-timelines>

In fact, that reasoning has a small hole in it. By examining the design of Tweet IDs, you will notice an edge case around its ordering and see that the common approach mentioned earlier would miss Tweets on rare occasions that should otherwise be processed, making the timeline "leaky". This article demonstrates how the problem of timeline leaks occurs and proposes an improved approach to polling timelines.

## Problem

Tweet IDs are generated by [Snowflake], an internal service developed by Twitter, which is used to generate IDs for other kinds of resources like users as well. The goal of Snowflake is to generate IDs parallelly across machines in an uncoordinated manner, to keep up with the scale of Twitter.

[Snowflake]: <https://github.com/twitter-archive/snowflake/tree/snowflake-2010>

Because of its uncoordinated nature, Snowflake cannot guarantee the chronological ordering of the IDs, as [described][snowflake-roughly-time-ordered] in the README of its GitHub repository. However, it still guarantees the IDs to be _roughly sorted_, or in mathematical terms, $k$-sorted. Regarding the value of $k$, the README says <q>we're promising 1s, but shooting for 10's of ms</q>, meaning that <q>tweets posted within a second of one another will be within a second of one another in the id space too</q>, according to the Twitter blog article _[Announcing Snowflake]_.

[snowflake-roughly-time-ordered]: <https://github.com/twitter-archive/snowflake/blob/snowflake-2010/README.mkd#roughly-time-ordered>
[Announcing Snowflake]: <https://blog.twitter.com/engineering/en_us/a/2010/announcing-snowflake>

To achieve the $k$-sortedness, Snowflake encodes a timestamp into the highest bits (`id >> 22`) of the generated ID, making sure that the IDs are ordered by the time of their generation in the respective machine's clock. The timestamp is of millisecond precision and is based on a custom epoch, which is [`twepoch = 1288834974657`][twepoch] milliseconds later from Unix epoch.

[twepoch]: <https://github.com/twitter-archive/snowflake/blob/snowflake-2010/src/main/scala/com/twitter/service/snowflake/IdWorker.scala#L25>

The $k$-sortedness works pretty well in most cases: as an end user, you wouldn't notice if the ordering of a timeline were different from a true chronological ordering by the magnitude of a mere 10 milliseconds. On the other hand, the common polling approach is not that tolerant. Remember that the common approach assumes a chronological ordering of the timelines, an assumption stronger than $k$-sortedness. For the approach to work correctly, the ID of the latest Tweet is required to be lower than any Tweets to be posted later. With the $k$-sortedness, however, Tweets with lower IDs may appear within $k = 1 \mathrm{s}$ of the latest Tweet, which would be skipped by the `since_id` parameter.

## Solution

The previous section showed that the conmon polling approach may miss Tweets that should otherwise be fetched, due to the lack of a complete ordering of Snowflake IDs. However, the $k$-sortedness of Snowflake IDs can guarantee an ordering between a pair of Tweets if the timestamps of the IDs are more than $k = 1 \mathrm{s}$ apart from each other. 

Now, let's pretend that we have a Teeet with an ID with its timestamp being $1 \mathrm{s}$ earlier than the ID of the latest Tweet. Then, such an ID is guaranteed to be lower than any Tweets to be posted after the latest Tweet so we can safely use that ID as the `since_id` parameter value to catch every future Tweet. Of course, we don't necessarily have such a Tweet, but we can calculate a (hypothetical) Snowflake ID with the same property using the following pseudo code (let `latest_id` be the ID of the latest Tweet):

```text
k = 1000
timestamp = latest_id >> 22
since_id =
    if timestamp <= k then
        /* Prevent overflow */
        latest_id
    else
        /* Subtract one because `since_id` parameter is exclusive */
        ((timestamp - k) << 22) - 1
```

The `since_id` value shown here is sufficient to prevent the problem of timeline leaks. However, as the pseudo ID value is lower than `latest_id` at least, this would cause same Tweets to be repeatedly returned by the API especially when the speed of the timeline is low. This is redundant when we know that the latest Tweet was posted more than $1 \mathrm{s}$ earlier than the time of the last request, since there is no risk of timeline leaks in that case.

Let's improve the earlier pseudo code by setting `since_id` parameter to `latest_id` as is if its timestamp is more than $1 \mathrm{s}$ earlier than the time of the last request. If we let `retrieved_at` be the millisecond precision Unix time of the last request, the code will look like the following:

```text
twepoch = 1288834974657
k = 1000

clamp(x, lower, upper) = max(lower, min(upper, x))
time2sf(unix_time_ms) = max(0, unix_time_ms - twepoch) << 22

/* Same calculation as the earlier code.
 * This value may be used when the local clock is behind Twitter's one. */
timestamp = latest_id >> 22
lower =
    if timestamp <= k then
        latest_id
    else
        ((timestamp - k) << 22) - 1

since_id = clamp(time2sf(retrieved_at - k) - 1, lower, latest_id)
```

Note that this code still fetches duplicate Tweets, which your app needs to handle, although the amount is now minimal.

## Concluding remarks

In this article, we have demonstrated the problem of _timeline leaks_, where client apps pollong a timeline may miss Tweets that should otherwise fetched due to $k$-sortedness of Snowflake IDs and proposed a solution to the problem.

However, the reasoning is purely theoretical so far. The [`experiment`](experiment/) directory contains an experimental code to see if the timeline leaks actually occur in Twitter API.

## License

See [`COPYING.md`](COPYING.md) for the copyright notice and license of this article and the experiment code.

<!-- vim: set linebreak: --> 
