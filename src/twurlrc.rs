use std::collections::HashMap;

use serde::de::{self, Error};
use serde::Deserialize;

pub struct DefaultProfile {
    pub username: String,
    pub bearer_token: String,
}

#[derive(Deserialize)]
struct Twurlrc {
    configuration: Configuration,
    bearer_tokens: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Configuration {
    default_profile: (String, String),
}

impl<'de> Deserialize<'de> for DefaultProfile {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let Twurlrc {
            configuration:
                Configuration {
                    default_profile: (username, consumer),
                },
            mut bearer_tokens,
        } = Twurlrc::deserialize(d)?;
        let bearer_token = if let Some(bearer_token) = bearer_tokens.remove(&consumer) {
            bearer_token
        } else {
            return Err(D::Error::custom("missing default app in `bearer_tokens`"));
        };

        Ok(DefaultProfile {
            username,
            bearer_token,
        })
    }
}
