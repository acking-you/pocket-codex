//! Validation of answers to OpenCode forms, before anything is submitted.

use std::collections::HashMap;

use serde_json::Value;

use super::{Error, Form, Result};

/// Whether this build can collect an answer for `field` (anything else must be
/// completed in OpenCode itself).
pub fn field_supported(field: &Value) -> bool {
    field["pattern"].is_null()
        && field["format"].is_null()
        && matches!(
            field["type"].as_str(),
            Some("string" | "number" | "integer" | "boolean" | "multiselect")
        )
}

/// Validate a typed answer dictionary against a pending form's fields.
pub fn validate_answer(form: &Form, answer: &Value) -> Result<()> {
    let answers = answer.as_object().ok_or(Error::InvalidInput)?;
    if form.fields.is_empty()
        || form.fields.len() > 100
        || answers.len() > 100
        || serde_json::to_vec(answer)
            .map_err(|_| Error::InvalidInput)?
            .len()
            > 64 * 1024
    {
        return Err(Error::InvalidInput);
    }
    let mut fields = HashMap::new();
    for field in &form.fields {
        let key = field["key"].as_str().ok_or(Error::Protocol)?;
        if fields.contains_key(key) {
            return Err(Error::Protocol);
        }
        // Unsupported constraints cannot silently become permissive answers.
        if !field_supported(field) {
            return Err(Error::InvalidInput);
        }
        let active = is_active(field, answers, &fields)?;
        fields.insert(key, field);
        let Some(value) = answers.get(key) else {
            if field["required"] == true && active {
                return Err(Error::InvalidInput);
            }
            continue;
        };
        if !active {
            return Err(Error::InvalidInput);
        }
        validate_value(field, value)?;
    }
    if answers.keys().any(|key| !fields.contains_key(key.as_str())) {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

fn is_active(
    field: &Value,
    answers: &serde_json::Map<String, Value>,
    earlier: &HashMap<&str, &Value>,
) -> Result<bool> {
    if field["when"].is_null() {
        return Ok(true);
    }
    let conditions = field["when"].as_array().ok_or(Error::Protocol)?;
    if conditions.len() > 100 {
        return Err(Error::Limit);
    }
    let mut active = true;
    for condition in conditions {
        let key = condition["key"].as_str().ok_or(Error::Protocol)?;
        let target = earlier.get(key).ok_or(Error::Protocol)?;
        let expected = &condition["value"];
        let expected_type = match target["type"].as_str() {
            Some("boolean") => expected.is_boolean(),
            Some("number" | "integer") => expected.is_number(),
            Some("string" | "multiselect") => {
                expected.is_string() && valid_option(target, expected)
            },
            _ => false,
        };
        if !expected_type || !matches!(condition["op"].as_str(), Some("eq" | "neq")) {
            return Err(Error::Protocol);
        }
        let Some(actual) = answers.get(key) else {
            active = false;
            continue;
        };
        let hit = match actual.as_array() {
            Some(values) => values.iter().any(|value| equal(value, expected)),
            None => equal(actual, expected),
        };
        active &= if condition["op"] == "eq" { hit } else { !hit };
    }
    Ok(active)
}

fn equal(left: &Value, right: &Value) -> bool {
    match (left.as_f64(), right.as_f64()) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

fn validate_value(field: &Value, value: &Value) -> Result<()> {
    match field["type"].as_str() {
        Some("boolean") if value.is_boolean() => Ok(()),
        Some("number" | "integer") => {
            let number = value.as_f64().ok_or(Error::InvalidInput)?;
            if !number.is_finite()
                || (field["type"] == "integer" && number.fract() != 0.0)
                || field["minimum"].as_f64().is_some_and(|min| number < min)
                || field["maximum"].as_f64().is_some_and(|max| number > max)
            {
                return Err(Error::InvalidInput);
            }
            Ok(())
        },
        Some("string") => {
            let text = value.as_str().ok_or(Error::InvalidInput)?;
            let length = text.encode_utf16().count() as u64;
            if (field["required"] == true && text.is_empty())
                || field["minLength"].as_u64().is_some_and(|min| length < min)
                || field["maxLength"].as_u64().is_some_and(|max| length > max)
                || !valid_option(field, value)
            {
                return Err(Error::InvalidInput);
            }
            Ok(())
        },
        Some("multiselect") => {
            let values = value.as_array().ok_or(Error::InvalidInput)?;
            if (field["required"] == true && values.is_empty())
                || field["minItems"]
                    .as_u64()
                    .is_some_and(|min| (values.len() as u64) < min)
                || field["maxItems"]
                    .as_u64()
                    .is_some_and(|max| (values.len() as u64) > max)
                || values
                    .iter()
                    .any(|value| !value.is_string() || !valid_option(field, value))
            {
                return Err(Error::InvalidInput);
            }
            Ok(())
        },
        _ => Err(Error::InvalidInput),
    }
}

fn valid_option(field: &Value, value: &Value) -> bool {
    if field["custom"] == true {
        return true;
    }
    match field.get("options") {
        Some(Value::Array(options)) => options.iter().any(|option| option["value"] == *value),
        Some(_) => false,
        None => field["type"] == "string",
    }
}
