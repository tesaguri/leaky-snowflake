use std::collections::HashMap;

use serde::de::{self, Error};
use serde::Deserialize;

pub struct DefaultProfile {
    pub username: String,
    pub token: oauth::Token,
}

#[derive(Deserialize)]
struct Twurlrc {
    profiles: HashMap<String, HashMap<String, Profile>>,
    configuration: Configuration,
}

#[derive(Deserialize)]
struct Profile {
    username: String,
    consumer_key: String,
    consumer_secret: String,
    token: String,
    secret: String,
}

#[derive(Deserialize)]
struct Configuration {
    default_profile: (String, String),
}

impl<'de> Deserialize<'de> for DefaultProfile {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Twurlrc {
            mut profiles,
            configuration:
                Configuration {
                    default_profile: (username, consumer),
                },
        } = Twurlrc::deserialize(d)?;
        let mut consumers = if let Some(consumers) = profiles.remove(&username) {
            consumers
        } else {
            return Err(D::Error::custom(format_args!(
                "missing default user @{} in `profiles`",
                username
            )));
        };
        let p = if let Some(profile) = consumers.remove(&consumer) {
            profile
        } else {
            return Err(D::Error::custom(format_args!(
                "missing default app in `profiles.{}`",
                username
            )));
        };

        let username = p.username;
        let token = oauth::Token::from_parts(p.consumer_key, p.consumer_secret, p.token, p.secret);

        Ok(DefaultProfile { username, token })
    }
}
