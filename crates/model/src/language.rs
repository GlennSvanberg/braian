use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// BCP 47 language with an optional region, for example `sv`, `sv-SE` or `en-GB`.
///
/// Script, variant and extension subtags are rejected. There is no fallback
/// between tags: `sv` and `sv-SE` are different languages.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn parse(raw: &str) -> Result<Self, LanguageTagError> {
        let bytes = raw.as_bytes();
        let lang_len = bytes
            .iter()
            .position(|byte| *byte == b'-')
            .unwrap_or(bytes.len());
        let language_ok =
            matches!(lang_len, 2 | 3) && bytes[..lang_len].iter().all(u8::is_ascii_lowercase);
        if !language_ok {
            return Err(LanguageTagError {
                tag: raw.to_owned(),
            });
        }
        if lang_len == bytes.len() {
            return Ok(Self(raw.to_owned()));
        }
        let region = &bytes[lang_len + 1..];
        let region_ok = (region.len() == 2 && region.iter().all(u8::is_ascii_uppercase))
            || (region.len() == 3 && region.iter().all(u8::is_ascii_digit));
        if region_ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(LanguageTagError {
                tag: raw.to_owned(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LanguageTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for LanguageTag {
    type Err = LanguageTagError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for LanguageTag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for LanguageTag {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .description(Some(
                "BCP 47 language tag with an optional region, for example sv or en-GB",
            ))
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for LanguageTag {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageTagError {
    tag: String,
}

impl std::fmt::Display for LanguageTagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid language tag '{}': expected a BCP 47 language with an optional region, such as sv or en-GB",
            self.tag
        )
    }
}

impl std::error::Error for LanguageTagError {}

#[cfg(test)]
mod tests {
    use super::LanguageTag;

    #[test]
    fn accepts_language_and_region() {
        for tag in ["sv", "en", "sv-SE", "en-GB", "es-419", "fil"] {
            assert!(LanguageTag::parse(tag).is_ok(), "{tag}");
        }
    }

    #[test]
    fn rejects_script_case_and_extensions() {
        for tag in ["", "e", "SV", "sv-se", "zh-Hans", "en-GB-x-private", "sv-"] {
            assert!(LanguageTag::parse(tag).is_err(), "{tag}");
        }
    }
}
