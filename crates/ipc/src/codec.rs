use std::collections::HashMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use zbus::wire::{OwnedVariant, Variant};

use crate::Error;

/// Native `a{sv}` on D-Bus. Optional fields are absent; all other values use
/// strings, booleans, numbers, arrays and nested dictionaries, never JSON text.
pub type State = HashMap<String, OwnedVariant>;

pub fn encode<T: Serialize>(value: &T) -> Result<State, Error> {
    let Value::Object(values) = serde_json::to_value(value)? else {
        return Err(Error::Invalid("expected a dictionary".into()));
    };
    values
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| {
            Ok((
                key,
                OwnedVariant::try_from(to_variant(value)?).map_err(Error::Bus)?,
            ))
        })
        .collect()
}

pub fn decode<T: DeserializeOwned>(state: State) -> Result<T, Error> {
    let object = state
        .into_iter()
        .map(|(key, value)| Ok((key, from_variant(&value)?)))
        .collect::<Result<serde_json::Map<_, _>, Error>>()?;
    Ok(serde_json::from_value(Value::Object(object))?)
}

fn to_variant(value: Value) -> Result<Variant<'static>, Error> {
    Ok(match value {
        Value::Bool(value) => Variant::Bool(value),
        Value::String(value) => Variant::Str(value.into()),
        Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                Variant::U64(value)
            } else if let Some(value) = value.as_i64() {
                Variant::I64(value)
            } else {
                Variant::F64(
                    value
                        .as_f64()
                        .ok_or_else(|| Error::Invalid("invalid numeric value".into()))?,
                )
            }
        }
        Value::Array(values) => {
            let values = values
                .into_iter()
                .map(|value| OwnedVariant::try_from(to_variant(value)?).map_err(Error::Bus))
                .collect::<Result<Vec<_>, Error>>()?;
            Variant::Array(values.into())
        }
        Value::Object(values) => {
            let values = values
                .into_iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| {
                    Ok((
                        key,
                        OwnedVariant::try_from(to_variant(value)?).map_err(Error::Bus)?,
                    ))
                })
                .collect::<Result<State, Error>>()?;
            Variant::Dict(values.into())
        }
        Value::Null => return Err(Error::Invalid("null array entries are unsupported".into())),
    })
}

fn from_variant(value: &Variant<'_>) -> Result<Value, Error> {
    Ok(match value {
        Variant::Bool(value) => Value::Bool(*value),
        Variant::Str(value) => Value::String(value.to_string()),
        Variant::U8(value) => Value::from(*value),
        Variant::U16(value) => Value::from(*value),
        Variant::U32(value) => Value::from(*value),
        Variant::U64(value) => Value::from(*value),
        Variant::I16(value) => Value::from(*value),
        Variant::I32(value) => Value::from(*value),
        Variant::I64(value) => Value::from(*value),
        Variant::F64(value) => Value::Number(
            serde_json::Number::from_f64(*value)
                .ok_or_else(|| Error::Invalid("non-finite numeric value".into()))?,
        ),
        Variant::Variant(value) => return from_variant(value),
        Variant::Array(values) => {
            Value::Array(values.iter().map(from_variant).collect::<Result<_, _>>()?)
        }
        Variant::Dict(values) => {
            let pairs = values
                .iter()
                .map(|(key, value)| {
                    let Variant::Str(key) = key else {
                        return Err(Error::Invalid("dictionary key must be a string".into()));
                    };
                    Ok((key.to_string(), from_variant(value)?))
                })
                .collect::<Result<serde_json::Map<_, _>, Error>>()?;
            Value::Object(pairs)
        }
        _ => return Err(Error::Invalid("unsupported dictionary value type".into())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use razerr_model::{Dpi, Percent, Snapshot, Speed};

    #[test]
    fn native_snapshot_roundtrip_without_device_fixtures() {
        let expected = Snapshot::default();
        let state = encode(&expected).expect("encode");
        let snapshot: Snapshot = decode(state).expect("decode");
        assert_eq!(snapshot.revision, expected.revision);
        assert_eq!(snapshot.kind, expected.kind);
        assert!(snapshot.devices.is_empty());
    }

    #[test]
    fn numeric_inputs_cannot_bypass_domain_constructors() {
        assert!(serde_json::from_str::<Percent>("101").is_err());
        assert!(serde_json::from_str::<Speed>("4").is_err());
        assert!(serde_json::from_str::<Dpi>(r#"{"x":0,"y":800}"#).is_err());
        let dpi = Dpi::new(900, 0).expect("DPI");
        assert_eq!(
            decode::<Dpi>(encode(&dpi).expect("encode")).expect("decode"),
            dpi
        );
    }

    #[test]
    fn nested_effect_dictionary_survives_actual_wire_encoding() {
        use razerr_model::{Effect, Rgb};
        use zbus::wire::{Endian, serialized::Context};
        let effect = Effect::StarlightDual {
            colors: [
                Rgb {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
                Rgb {
                    red: 4,
                    green: 5,
                    blue: 6,
                },
            ],
            speed: Speed::new(2).expect("speed"),
        };
        let encoded = encode(&effect).expect("encode native dict");
        let bytes = zbus::wire::to_bytes(Context::new(Endian::Little, 0), &encoded)
            .expect("serialize D-Bus");
        let (state, _): (State, _) = bytes.deserialize().expect("deserialize D-Bus");
        assert_eq!(
            decode::<Effect>(state).expect("validate domain value"),
            effect
        );
    }
}
