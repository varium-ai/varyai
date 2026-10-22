//! Reject duplicate JSON keys before inspecting a request: parsers differ in
//! which duplicate wins, so inspecting one and forwarding both is unsafe.
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{Map, Value};
use std::fmt;

struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = object.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                    let UniqueValue(value) = object.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<UniqueValue>(bytes).map(|value| value.0)
}

/// Only encode edited text leaves. Opaque values keep their original lexemes,
/// including integers beyond u64 and decimals beyond f64 precision. Refuse any
/// unexpected structural edit instead of silently discarding protocol fields.
pub(super) fn encode_redacted(
    raw: &[u8],
    original: &Value,
    redacted: &Value,
) -> Result<Vec<u8>, &'static str> {
    let raw: &serde_json::value::RawValue =
        serde_json::from_slice(raw).map_err(|_| "pi_route_invalid_json")?;
    let mut output = Vec::new();
    append_redacted(&mut output, raw, original, redacted)?;
    Ok(output)
}

fn append_redacted(
    output: &mut Vec<u8>,
    raw: &serde_json::value::RawValue,
    original: &Value,
    redacted: &Value,
) -> Result<(), &'static str> {
    if original == redacted {
        output.extend_from_slice(raw.get().as_bytes());
        return Ok(());
    }
    let invalid = "pi_route_unsafe_redaction";
    match (original, redacted) {
        (Value::String(_), Value::String(_)) => {
            serde_json::to_writer(output, redacted).map_err(|_| invalid)
        }
        (Value::Object(before), Value::Object(after)) if before.len() == after.len() => {
            let raw_values: std::collections::HashMap<String, &serde_json::value::RawValue> =
                serde_json::from_str(raw.get()).map_err(|_| invalid)?;
            output.push(b'{');
            for (index, (key, old)) in before.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key).map_err(|_| invalid)?;
                output.push(b':');
                append_redacted(
                    output,
                    raw_values.get(key).ok_or(invalid)?,
                    old,
                    after.get(key).ok_or(invalid)?,
                )?;
            }
            output.push(b'}');
            Ok(())
        }
        (Value::Array(before), Value::Array(after)) if before.len() == after.len() => {
            let raw_values: Vec<&serde_json::value::RawValue> =
                serde_json::from_str(raw.get()).map_err(|_| invalid)?;
            if raw_values.len() != before.len() {
                return Err(invalid);
            }
            output.push(b'[');
            for (index, ((raw, old), new)) in raw_values.iter().zip(before).zip(after).enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                append_redacted(output, raw, old, new)?;
            }
            output.push(b']');
            Ok(())
        }
        _ => Err(invalid),
    }
}
