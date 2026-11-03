use serde::{Deserialize, Deserializer};
use thiserror::Error;

pub(crate) mod answer_checks;
mod chat;
mod loupist;
mod memory;

pub use answer_checks::{
    valid_as_of, AsOfEmpty, AsOfLeftOut, AsOfRefusal, AsOfReport, CalcInputUnit, CalcInputUnitKind,
    NumberCheck, NumberCheckCalc, NumberCheckField, NumberCheckInput, NumberCheckKind,
    NumberCheckStatus, SensitivityCard, SensitivityGrid, SensitivityInput, SensitivityPrice,
    SensitivityReference, MAX_AS_OF_ITEMS, MAX_NUMBER_CHECKS, MAX_SENSITIVITY_CARDS,
};
pub use chat::*;
pub use loupist::*;
pub use memory::*;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid domain value: {0}")]
    Invalid(&'static str),
}

pub(crate) fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn require_bounded_text(
    value: &str,
    maximum: usize,
    message: &'static str,
) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(DomainError::Invalid(message));
    }
    Ok(())
}

fn require_identifier(value: &str, message: &'static str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(DomainError::Invalid(message));
    }
    Ok(())
}

fn validate_sha256_digest(value: &str, message: &'static str) -> Result<(), DomainError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DomainError::Invalid(message));
    }
    Ok(())
}
