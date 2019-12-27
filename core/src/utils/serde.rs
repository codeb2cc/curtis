use std::fmt::Display;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

pub fn de_str2num<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + FromStr + serde::Deserialize<'de>,
    <T as FromStr>::Err: Display,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber<T> {
        String(String),
        Number(T),
    }

    match StringOrNumber::<T>::deserialize(deserializer)? {
        StringOrNumber::String(s) => {
            if s.is_empty() {
                Ok(T::default())
            } else {
                s.parse::<T>().map_err(serde::de::Error::custom)
            }
        }
        StringOrNumber::Number(i) => Ok(i),
    }
}
