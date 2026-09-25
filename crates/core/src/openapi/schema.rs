//! JSON Schema (2020-12, as OpenAPI 3.1 uses it) inferred from observed JSON values,
//! merged across samples: a property is required when every object seen had it, a type
//! is a list when samples disagree (`["string", "null"]`), integers and numbers merge to
//! `number`, and a string format (`date-time`, `uuid`, `email`, `uri`, `date`) is kept only
//! when every string had it. Values themselves are never copied into the schema.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

/// How deep objects and arrays are followed (deeper values count as anything).
const MAX_DEPTH: usize = 12;
/// Properties kept per object (the most frequent).
const MAX_PROPERTIES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

/// A string format recognised in values.
pub(crate) fn string_format(value: &str) -> Option<&'static str> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|b| b.iter().all(u8::is_ascii_digit))
    };
    let is_date = value.len() >= 10
        && digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10);
    if is_date && value.len() == 10 {
        return Some("date");
    }
    if is_date
        && matches!(bytes.get(10), Some(b'T' | b't' | b' '))
        && digits(11..13)
        && bytes.get(13) == Some(&b':')
    {
        return Some("date-time");
    }
    if is_uuid(value) {
        return Some("uuid");
    }
    if let Some((user, domain)) = value.split_once('@')
        && !user.is_empty()
        && domain.contains('.')
        && !value.contains(char::is_whitespace)
        && !domain.starts_with('.')
        && !domain.ends_with('.')
    {
        return Some("email");
    }
    if (value.starts_with("https://") || value.starts_with("http://"))
        && !value.contains(char::is_whitespace)
    {
        return Some("uri");
    }
    None
}

/// `8-4-4-4-12` hexadecimal.
pub(crate) fn is_uuid(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A schema being learned from samples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Learned {
    kinds: BTreeSet<Kind>,
    /// Formats of the strings seen (`None` in the set: a string without one).
    formats: BTreeSet<Option<&'static str>>,
    /// Objects seen, and per property how often it appeared and its schema.
    objects: u64,
    properties: BTreeMap<String, (u64, Learned)>,
    /// Arrays' items.
    items: Option<Box<Learned>>,
    /// Anything below the depth limit.
    opaque: bool,
}

impl Learned {
    /// Learns from one more sample.
    pub fn add(&mut self, value: &Value) {
        self.add_at(value, 0);
    }

    fn add_at(&mut self, value: &Value, depth: usize) {
        if depth >= MAX_DEPTH {
            self.opaque = true;
            return;
        }
        match value {
            Value::Null => {
                self.kinds.insert(Kind::Null);
            }
            Value::Bool(_) => {
                self.kinds.insert(Kind::Boolean);
            }
            Value::Number(n) => {
                self.kinds.insert(if n.is_i64() || n.is_u64() {
                    Kind::Integer
                } else {
                    Kind::Number
                });
            }
            Value::String(s) => {
                self.kinds.insert(Kind::String);
                self.formats.insert(string_format(s));
            }
            Value::Array(items) => {
                self.kinds.insert(Kind::Array);
                let learned = self.items.get_or_insert_with(Box::default);
                for item in items {
                    learned.add_at(item, depth + 1);
                }
            }
            Value::Object(map) => {
                self.kinds.insert(Kind::Object);
                self.objects += 1;
                for (key, value) in map {
                    if !self.properties.contains_key(key) && self.properties.len() >= MAX_PROPERTIES
                    {
                        continue;
                    }
                    let entry = self.properties.entry(key.clone()).or_default();
                    entry.0 += 1;
                    entry.1.add_at(value, depth + 1);
                }
            }
        }
    }

    /// Whether nothing was learned.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && !self.opaque
    }

    /// The schema.
    pub fn schema(&self) -> Value {
        if self.opaque || self.kinds.is_empty() {
            return json!({});
        }
        let mut kinds = self.kinds.clone();
        if kinds.contains(&Kind::Number) {
            kinds.remove(&Kind::Integer);
        }
        let mut schema = Map::new();
        let names: Vec<Value> = kinds.iter().map(|k| json!(k.name())).collect();
        schema.insert(
            "type".into(),
            match names.as_slice() {
                [one] => one.clone(),
                _ => Value::Array(names),
            },
        );
        if kinds.contains(&Kind::String)
            && self.formats.len() == 1
            && let Some(Some(format)) = self.formats.iter().next()
        {
            schema.insert("format".into(), json!(format));
        }
        if kinds.contains(&Kind::Object) {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for (name, (count, learned)) in &self.properties {
                properties.insert(name.clone(), learned.schema());
                if *count == self.objects {
                    required.push(json!(name));
                }
            }
            schema.insert("properties".into(), Value::Object(properties));
            if !required.is_empty() {
                schema.insert("required".into(), Value::Array(required));
            }
        }
        if kinds.contains(&Kind::Array) {
            schema.insert(
                "items".into(),
                self.items
                    .as_ref()
                    .filter(|items| !items.is_empty())
                    .map_or_else(|| json!({}), |items| items.schema()),
            );
        }
        Value::Object(schema)
    }
}

/// The schema of a scalar parameter (path, query, header) from its observed values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scalar {
    kinds: BTreeSet<Kind>,
    formats: BTreeSet<Option<&'static str>>,
}

impl Scalar {
    /// Learns from one more value.
    pub fn add(&mut self, value: &str) {
        let kind = if !value.is_empty() && value.parse::<i64>().is_ok() {
            Kind::Integer
        } else if value.parse::<f64>().is_ok_and(f64::is_finite) && !value.is_empty() {
            Kind::Number
        } else if value == "true" || value == "false" {
            Kind::Boolean
        } else {
            Kind::String
        };
        self.kinds.insert(kind);
        if kind == Kind::String {
            self.formats.insert(string_format(value));
        }
    }

    /// The schema: one type when all values agree, else `string`.
    pub fn schema(&self) -> Value {
        let kinds: Vec<Kind> = self.kinds.iter().copied().collect();
        let kind = match kinds.as_slice() {
            [one] => *one,
            [Kind::Integer, Kind::Number] => Kind::Number,
            _ => Kind::String,
        };
        let mut schema = json!({ "type": kind.name() });
        if kind == Kind::String
            && self.formats.len() == 1
            && let Some(Some(format)) = self.formats.iter().next()
        {
            schema["format"] = json!(format);
        }
        schema
    }
}

/// Whether `value` conforms to `schema` (the subset of JSON Schema [`Learned`] writes:
/// `type`, `properties`, `required`, `items`, `format`). For tests and checks.
pub fn conforms(value: &Value, schema: &Value) -> bool {
    let Some(schema) = schema.as_object() else {
        return false;
    };
    let types: Vec<&str> = match schema.get("type") {
        None => return true,
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        Some(_) => return false,
    };
    match value {
        Value::Null => types.contains(&"null"),
        Value::Bool(_) => types.contains(&"boolean"),
        Value::Number(n) => {
            types.contains(&"number") || (types.contains(&"integer") && (n.is_i64() || n.is_u64()))
        }
        Value::String(s) => {
            types.contains(&"string")
                && schema
                    .get("format")
                    .and_then(Value::as_str)
                    .is_none_or(|f| string_format(s) == Some(f))
        }
        Value::Array(items) => {
            types.contains(&"array")
                && schema
                    .get("items")
                    .is_none_or(|item_schema| items.iter().all(|i| conforms(i, item_schema)))
        }
        Value::Object(map) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            let required_ok = schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|req| {
                    req.iter()
                        .filter_map(Value::as_str)
                        .all(|k| map.contains_key(k))
                });
            types.contains(&"object")
                && required_ok
                && map.iter().all(|(k, v)| {
                    properties
                        .and_then(|p| p.get(k))
                        .is_none_or(|s| conforms(v, s))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn merges_objects_types_and_formats() {
        let mut learned = Learned::default();
        learned.add(&json!({"id": 1, "email": "a@b.co", "tags": ["x"], "note": null}));
        learned.add(&json!({"id": 2.5, "email": "c@d.io", "tags": [], "note": "hi"}));
        learned.add(&json!({"id": 3, "email": "e@f.dev", "tags": ["y", "z"]}));
        let schema = learned.schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["id"]["type"], "number");
        assert_eq!(schema["properties"]["email"]["format"], "email");
        assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");
        assert_eq!(
            schema["properties"]["note"]["type"],
            json!(["null", "string"])
        );
        assert_eq!(schema["required"], json!(["email", "id", "tags"]));
    }

    #[test]
    fn formats_need_every_string() {
        let mut learned = Learned::default();
        learned.add(&json!("2026-09-25T10:00:00Z"));
        assert_eq!(learned.schema()["format"], "date-time");
        learned.add(&json!("tomorrow"));
        assert!(learned.schema().get("format").is_none());
        assert_eq!(string_format("2026-09-25"), Some("date"));
        assert_eq!(
            string_format("123e4567-e89b-12d3-a456-426614174000"),
            Some("uuid")
        );
        assert_eq!(string_format("https://example.com/x"), Some("uri"));
        assert_eq!(string_format("hello world"), None);
    }

    #[test]
    fn scalars() {
        let mut id = Scalar::default();
        id.add("42");
        id.add("7");
        assert_eq!(id.schema(), json!({"type": "integer"}));
        id.add("1.5");
        assert_eq!(id.schema(), json!({"type": "number"}));
        id.add("abc");
        assert_eq!(id.schema(), json!({"type": "string"}));
        let mut flag = Scalar::default();
        flag.add("true");
        assert_eq!(flag.schema(), json!({"type": "boolean"}));
    }

    fn json_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i32>().prop_map(|n| json!(n)),
            (-1e6f64..1e6).prop_map(|n| json!(n)),
            "[a-z0-9@.:/ -]{0,20}".prop_map(Value::String),
            Just(json!("2026-09-25T10:00:00Z")),
            Just(json!("123e4567-e89b-12d3-a456-426614174000")),
        ];
        leaf.prop_recursive(4, 32, 5, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
                proptest::collection::btree_map("[a-e]{1,3}", inner, 0..5)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    proptest! {
        /// Every sample conforms to the schema learned from all of them.
        #[test]
        fn every_sample_conforms(samples in proptest::collection::vec(json_value(), 1..8)) {
            let mut learned = Learned::default();
            for sample in &samples {
                learned.add(sample);
            }
            let schema = learned.schema();
            for sample in &samples {
                prop_assert!(conforms(sample, &schema), "{sample} vs {schema}");
            }
        }

        /// The order samples arrive in doesn't change the schema.
        #[test]
        fn order_doesnt_matter(samples in proptest::collection::vec(json_value(), 1..6)) {
            let mut forward = Learned::default();
            let mut backward = Learned::default();
            for sample in &samples {
                forward.add(sample);
            }
            for sample in samples.iter().rev() {
                backward.add(sample);
            }
            prop_assert_eq!(forward.schema(), backward.schema());
        }
    }
}
