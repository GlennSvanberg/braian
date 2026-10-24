use serde::{Deserialize, Serialize};

use crate::id::AttributeId;
use crate::language::LanguageTag;

/// Machine-readable validation failure.
///
/// Messages are for people. Callers branch on `code`, together with the
/// attribute, language and list index that locate the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownType,
    UnknownStructure,
    UnknownGroup,
    AttributeNotApplicable,
    WrongType,
    ExpectedList,
    ExpectedLanguageMap,
    LanguageNotAllowed,
    TextTooLong,
    InvalidDecimal,
    UnitRequired,
    UnitNotAllowed,
    BelowMin,
    AboveMax,
    ValueNotInList,
    InvalidDate,
    InvalidFileRef,
    MissingKeyAttribute,
}

impl ErrorCode {
    pub const ALL: [Self; 18] = [
        Self::UnknownType,
        Self::UnknownStructure,
        Self::UnknownGroup,
        Self::AttributeNotApplicable,
        Self::WrongType,
        Self::ExpectedList,
        Self::ExpectedLanguageMap,
        Self::LanguageNotAllowed,
        Self::TextTooLong,
        Self::InvalidDecimal,
        Self::UnitRequired,
        Self::UnitNotAllowed,
        Self::BelowMin,
        Self::AboveMax,
        Self::ValueNotInList,
        Self::InvalidDate,
        Self::InvalidFileRef,
        Self::MissingKeyAttribute,
    ];
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = serde_json::to_value(self).expect("error code serializes");
        let Some(text) = code.as_str() else {
            return Err(std::fmt::Error);
        };
        f.write_str(text)
    }
}

/// One validation failure, located as precisely as the value allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<AttributeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    pub code: ErrorCode,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    #[test]
    fn codes_serialize_snake_case() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::TextTooLong).unwrap(),
            "\"text_too_long\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::MissingKeyAttribute).unwrap(),
            "\"missing_key_attribute\""
        );
    }
}
