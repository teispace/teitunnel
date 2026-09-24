//! A YAML document as a tree of values that remember where they were written, so every
//! problem in a project file can point at its line and column.

use std::fmt;

use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_saphyr::Spanned;

/// A 1-based line and column in the file (0 when unknown).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pos {
    /// Line.
    pub line: u32,
    /// Column.
    pub column: u32,
}

/// A value and where it starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node {
    pub(crate) value: Value,
    pub(crate) at: Pos,
}

/// A YAML value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

impl Node {
    /// Text, or a number written as text (`origin: 3000`).
    pub(crate) fn scalar(&self) -> Option<String> {
        match &self.value {
            Value::Str(s) => Some(s.clone()),
            Value::Int(n) => Some(n.to_string()),
            Value::Float(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Null | Value::Seq(_) | Value::Map(_) => None,
        }
    }

    /// The entries of a mapping with text keys.
    pub(crate) fn entries(&self) -> Option<Vec<(&str, Pos, &Node)>> {
        match &self.value {
            Value::Map(entries) => Some(
                entries
                    .iter()
                    .filter_map(|(key, value)| match &key.value {
                        Value::Str(name) => Some((name.as_str(), key.at, value)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// Every text value in the tree (with where it is and the key it's under).
    pub(crate) fn strings<'a>(
        &'a self,
        key: Option<&'a str>,
        out: &mut Vec<(&'a str, &'a str, Pos)>,
    ) {
        match &self.value {
            Value::Str(s) => out.push((key.unwrap_or_default(), s, self.at)),
            Value::Seq(items) => {
                for item in items {
                    item.strings(key, out);
                }
            }
            Value::Map(entries) => {
                for (k, v) in entries {
                    let name = match &k.value {
                        Value::Str(name) => Some(name.as_str()),
                        _ => key,
                    };
                    v.strings(name, out);
                }
            }
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {}
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spanned = Spanned::<Value>::deserialize(deserializer)?;
        let at = Pos {
            line: u32::try_from(spanned.referenced.line()).unwrap_or(0),
            column: u32::try_from(spanned.referenced.column()).unwrap_or(0),
        };
        Ok(Self {
            value: spanned.value,
            at,
        })
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a YAML value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(i64::try_from(v).map_or(Value::Str(v.to_string()), Value::Int))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Node>()? {
            items.push(item);
        }
        Ok(Value::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key::<Node>()? {
            let value = map.next_value::<Node>()?;
            entries.push((key, value));
        }
        Ok(Value::Map(entries))
    }
}

/// A syntax error: where and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxError {
    pub(crate) at: Pos,
    pub(crate) message: String,
}

/// Parses a YAML document (an empty one is `Null`).
pub(crate) fn parse(text: &str) -> Result<Node, SyntaxError> {
    if text.trim().is_empty() {
        return Ok(Node {
            value: Value::Null,
            at: Pos { line: 1, column: 1 },
        });
    }
    serde_saphyr::from_str::<Node>(text).map_err(|err| {
        let at = err.location().map_or_else(Pos::default, |l| Pos {
            line: u32::try_from(l.line()).unwrap_or(0),
            column: u32::try_from(l.column()).unwrap_or(0),
        });
        // The first line of the message, without the location it repeats.
        let message = err
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        SyntaxError { at, message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_where_values_are() {
        let doc =
            parse("version: 1\nroutes:\n  - hostname: a.example.com\n    origin: 3000\n").unwrap();
        let entries = doc.entries().unwrap();
        assert_eq!(entries[0].0, "version");
        assert_eq!(entries[0].1, Pos { line: 1, column: 1 });
        let Value::Seq(routes) = &entries[1].2.value else {
            panic!("not a list");
        };
        let route = routes[0].entries().unwrap();
        assert_eq!(route[0].1, Pos { line: 3, column: 5 });
        assert_eq!(route[1].2.scalar().as_deref(), Some("3000"));
        assert_eq!(
            route[1].2.at,
            Pos {
                line: 4,
                column: 13
            }
        );
    }

    #[test]
    fn reports_syntax_errors_with_a_position() {
        let err = parse("routes:\n  - hostname: [unclosed\n").unwrap_err();
        assert!(err.at.line >= 2, "{err:?}");
        assert!(!err.message.is_empty());
    }

    #[test]
    fn empty_is_null() {
        assert_eq!(parse("  \n").unwrap().value, Value::Null);
    }
}
