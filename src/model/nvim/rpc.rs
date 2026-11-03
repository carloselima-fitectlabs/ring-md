//! msgpack-RPC message framing (the protocol Neovim speaks over stdio).
//!
//! Pure functions only: encoding to bytes and classifying decoded values.
//! The transport lives in [`super::session`].

use rmpv::Value;

const REQUEST: u64 = 0;
const RESPONSE: u64 = 1;
const NOTIFICATION: u64 = 2;

/// A decoded msgpack-RPC message.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Request {
        id: u64,
        method: String,
        params: Vec<Value>,
    },
    Response {
        id: u64,
        result: Result<Value, Value>,
    },
    Notification {
        method: String,
        params: Vec<Value>,
    },
}

/// Why a value could not be read as a [`Message`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed msgpack-RPC message: {0}")]
pub struct MalformedMessage(pub String);

impl Message {
    /// Classify a decoded msgpack value.
    pub fn from_value(value: Value) -> Result<Self, MalformedMessage> {
        let malformed = |what: &str| MalformedMessage(what.to_owned());
        let Value::Array(items) = value else {
            return Err(malformed("not an array"));
        };
        let mut items = items.into_iter();
        let kind = items
            .next()
            .and_then(|v| v.as_u64())
            .ok_or_else(|| malformed("missing type"))?;
        match kind {
            REQUEST => {
                let id = next_u64(&mut items).ok_or_else(|| malformed("request id"))?;
                let method = next_string(&mut items).ok_or_else(|| malformed("request method"))?;
                let params = next_array(&mut items).ok_or_else(|| malformed("request params"))?;
                Ok(Self::Request { id, method, params })
            }
            RESPONSE => {
                let id = next_u64(&mut items).ok_or_else(|| malformed("response id"))?;
                let error = items.next().unwrap_or(Value::Nil);
                let result = items.next().unwrap_or(Value::Nil);
                let result = if error.is_nil() {
                    Ok(result)
                } else {
                    Err(error)
                };
                Ok(Self::Response { id, result })
            }
            NOTIFICATION => {
                let method =
                    next_string(&mut items).ok_or_else(|| malformed("notification method"))?;
                let params =
                    next_array(&mut items).ok_or_else(|| malformed("notification params"))?;
                Ok(Self::Notification { method, params })
            }
            other => Err(MalformedMessage(format!("unknown type {other}"))),
        }
    }

    /// The msgpack value for this message.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Request { id, method, params } => Value::Array(vec![
                REQUEST.into(),
                (*id).into(),
                method.as_str().into(),
                Value::Array(params.clone()),
            ]),
            Self::Response { id, result } => {
                let (error, result) = match result {
                    Ok(value) => (Value::Nil, value.clone()),
                    Err(error) => (error.clone(), Value::Nil),
                };
                Value::Array(vec![RESPONSE.into(), (*id).into(), error, result])
            }
            Self::Notification { method, params } => Value::Array(vec![
                NOTIFICATION.into(),
                method.as_str().into(),
                Value::Array(params.clone()),
            ]),
        }
    }

    /// Serialize to msgpack bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Writing to a Vec cannot fail.
        let _ = rmpv::encode::write_value(&mut bytes, &self.to_value());
        bytes
    }
}

fn next_u64(items: &mut impl Iterator<Item = Value>) -> Option<u64> {
    items.next()?.as_u64()
}

fn next_string(items: &mut impl Iterator<Item = Value>) -> Option<String> {
    match items.next()? {
        Value::String(s) => s.into_str(),
        _ => None,
    }
}

fn next_array(items: &mut impl Iterator<Item = Value>) -> Option<Vec<Value>> {
    match items.next()? {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

/// Human-readable text for an error value returned by Neovim
/// (`[type, "message"]`).
pub fn error_text(error: &Value) -> String {
    match error {
        Value::Array(items) => items
            .iter()
            .find_map(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| error.to_string()),
        Value::String(s) => s.as_str().unwrap_or_default().to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(message: Message) {
        let bytes = message.encode();
        let value = rmpv::decode::read_value(&mut bytes.as_slice()).unwrap();
        assert_eq!(Message::from_value(value).unwrap(), message);
    }

    #[test]
    fn roundtrips_all_kinds() {
        roundtrip(Message::Request {
            id: 7,
            method: "nvim_input".into(),
            params: vec!["ihello".into()],
        });
        roundtrip(Message::Response {
            id: 7,
            result: Ok(Value::from(3)),
        });
        roundtrip(Message::Response {
            id: 8,
            result: Err(Value::Array(vec![0.into(), "boom".into()])),
        });
        roundtrip(Message::Notification {
            method: "redraw".into(),
            params: vec![Value::Array(vec!["flush".into()])],
        });
    }

    #[test]
    fn rejects_garbage() {
        assert!(Message::from_value(Value::from(1)).is_err());
        assert!(Message::from_value(Value::Array(vec![9.into()])).is_err());
        assert!(Message::from_value(Value::Array(vec![0.into(), "x".into()])).is_err());
    }

    #[test]
    fn error_text_extracts_message() {
        let error = Value::Array(vec![1.into(), "Vim:E492: Not an editor command".into()]);
        assert_eq!(error_text(&error), "Vim:E492: Not an editor command");
    }
}
