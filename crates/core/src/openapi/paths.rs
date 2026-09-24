//! Path templates from observed paths: segments that look like identifiers (numbers,
//! UUIDs, long hexadecimal or random tokens, dates) become parameters, and so do
//! positions where many different one-off values appear under the same parent (slugs):
//! `/users/123/orders/9f2c…` → `/users/{userId}/orders/{id}`.

use std::collections::{BTreeMap, BTreeSet};

use super::schema::is_uuid;

/// Distinct one-off values under one parent that make the position a parameter.
const SLUG_CLUSTER: usize = 8;

/// A template segment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Segment {
    /// Literal text.
    Static(String),
    /// A parameter (named when the template is finished).
    Param,
}

/// Whether a path segment looks like an identifier rather than a fixed name.
pub fn looks_dynamic(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    let digits = segment.chars().filter(char::is_ascii_digit).count();
    let letters = segment.chars().filter(char::is_ascii_alphabetic).count();
    // 123, 00042
    if digits == segment.len() {
        return true;
    }
    if is_uuid(segment) {
        return true;
    }
    // 2026-09-25
    if segment.len() == 10 && super::schema::string_format(segment) == Some("date") {
        return true;
    }
    // Object ids and hashes: hexadecimal with digits, 8 characters or more.
    if segment.len() >= 8 && digits > 0 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Random tokens: long, mixing letters and digits (`cus_N1a2b3c4d5`, base64url).
    segment.len() >= 16
        && digits >= 2
        && letters >= 2
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Splits a path into segments (no empty ones; a trailing slash is dropped).
pub(super) fn split(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect()
}

fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = segment.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Templates for a set of paths, and which template each path belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Templates {
    /// Templates as segment lists.
    pub templates: BTreeSet<Vec<Segment>>,
}

impl Templates {
    /// Learns templates from `paths` (each split with [`split`]).
    pub fn learn<'a>(paths: impl IntoIterator<Item = &'a [String]>) -> Self {
        let paths: Vec<&[String]> = paths.into_iter().collect();
        // First pass: identifiers.
        let generalized: Vec<Vec<Segment>> = paths
            .iter()
            .map(|path| {
                path.iter()
                    .map(|s| {
                        if looks_dynamic(s) {
                            Segment::Param
                        } else {
                            Segment::Static(s.clone())
                        }
                    })
                    .collect()
            })
            .collect();
        // Second pass: positions with many different leaf values under one parent.
        let mut siblings: BTreeMap<(Vec<Segment>, usize), BTreeSet<String>> = BTreeMap::new();
        for path in &generalized {
            for (index, segment) in path.iter().enumerate() {
                if let Segment::Static(value) = segment {
                    let shape: Vec<Segment> = path
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            if i == index {
                                Segment::Param
                            } else {
                                s.clone()
                            }
                        })
                        .collect();
                    siblings
                        .entry((shape, index))
                        .or_default()
                        .insert(value.clone());
                }
            }
        }
        let templates = generalized
            .into_iter()
            .map(|path| {
                let mut path = path;
                for index in 0..path.len() {
                    if !matches!(path[index], Segment::Static(_)) {
                        continue;
                    }
                    let shape: Vec<Segment> = path
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            if i == index {
                                Segment::Param
                            } else {
                                s.clone()
                            }
                        })
                        .collect();
                    if index > 0 && siblings.get(&(shape, index)).is_some_and(is_slug_cluster) {
                        path[index] = Segment::Param;
                    }
                }
                path
            })
            .collect();
        Self { templates }
    }

    /// The template `path` belongs to (the most specific that matches).
    pub fn find(&self, path: &[String]) -> Option<&Vec<Segment>> {
        self.templates
            .iter()
            .filter(|template| matches(template, path))
            .max_by_key(|template| {
                template
                    .iter()
                    .filter(|s| matches!(s, Segment::Static(_)))
                    .count()
            })
    }
}

/// Many different values, mostly shaped like slugs (`hello-world`, `v2_notes`, `post-1`),
/// under the same parent: a parameter. A handful of plain words (`intro`, `setup`)
/// stay separate paths.
fn is_slug_cluster(values: &BTreeSet<String>) -> bool {
    let slugs = values
        .iter()
        .filter(|v| v.contains(['-', '_', '.']) || v.chars().any(|c| c.is_ascii_digit()))
        .count();
    values.len() >= SLUG_CLUSTER && slugs * 2 >= values.len()
}

/// Whether `path` fits `template`.
pub(super) fn matches(template: &[Segment], path: &[String]) -> bool {
    template.len() == path.len()
        && template.iter().zip(path).all(|(t, p)| match t {
            Segment::Static(s) => s == p,
            Segment::Param => true,
        })
}

fn singular(word: &str) -> String {
    let lower: String = word
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix("ies") {
        format!("{stem}y")
    } else if lower.ends_with("ss") {
        lower
    } else if let Some(stem) = lower.strip_suffix('s') {
        stem.to_owned()
    } else {
        lower
    }
}

/// Parameter names for a template: the last one is `id`, earlier ones are named after
/// the segment before them (`users` → `userId`); names are unique within the path.
pub(super) fn param_names(template: &[Segment]) -> Vec<String> {
    let params: Vec<usize> = template
        .iter()
        .enumerate()
        .filter(|(_, s)| **s == Segment::Param)
        .map(|(i, _)| i)
        .collect();
    let mut names: Vec<String> = Vec::new();
    for (n, index) in params.iter().enumerate() {
        let before = index
            .checked_sub(1)
            .and_then(|i| template.get(i))
            .and_then(|s| match s {
                Segment::Static(text) => Some(singular(text)),
                Segment::Param => None,
            })
            .filter(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()));
        let base = if n + 1 == params.len() {
            "id".to_owned()
        } else {
            before.map_or_else(|| "param".to_owned(), |word| format!("{word}Id"))
        };
        let mut name = base.clone();
        let mut count = 2;
        while names.contains(&name) {
            name = format!("{base}{count}");
            count += 1;
        }
        names.push(name);
    }
    names
}

/// The template as an OpenAPI path, e.g. `/users/{userId}/orders/{id}`.
pub(super) fn render(template: &[Segment]) -> String {
    let names = param_names(template);
    let mut names = names.iter();
    let mut out = String::new();
    for segment in template {
        out.push('/');
        match segment {
            Segment::Static(text) => out.push_str(&text.replace(['{', '}'], "")),
            Segment::Param => {
                out.push('{');
                out.push_str(names.next().map_or("id", String::as_str));
                out.push('}');
            }
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn learn(paths: &[&str]) -> Vec<String> {
        let split: Vec<Vec<String>> = paths.iter().map(|p| split(p)).collect();
        let templates = Templates::learn(split.iter().map(Vec::as_slice));
        templates.templates.iter().map(|t| render(t)).collect()
    }

    #[test]
    fn identifiers_become_parameters() {
        assert_eq!(
            learn(&["/users/123", "/users/456", "/users"]),
            ["/users", "/users/{id}"]
        );
        assert_eq!(
            learn(&["/users/7/orders/123e4567-e89b-12d3-a456-426614174000"]),
            ["/users/{userId}/orders/{id}"]
        );
        assert_eq!(
            learn(&["/categories/12/items/5"]),
            ["/categories/{categoryId}/items/{id}"]
        );
        assert_eq!(learn(&["/", "/health"]), ["/", "/health"]);
        assert_eq!(
            learn(&["/objects/507f1f77bcf86cd799439011"]),
            ["/objects/{id}"]
        );
        assert_eq!(
            learn(&["/v1/customers/cus_N1a2B3c4D5e6F7g8"]),
            ["/v1/customers/{id}"]
        );
    }

    #[test]
    fn many_one_off_names_under_a_parent_are_slugs() {
        let posts: Vec<String> = (0..SLUG_CLUSTER)
            .map(|i| {
                format!(
                    "/posts/hello-world-part-{}",
                    ["a", "b", "c", "d", "e", "f", "g", "h", "i"][i]
                )
            })
            .collect();
        let mut paths: Vec<&str> = posts.iter().map(String::as_str).collect();
        paths.push("/about");
        paths.push("/docs/intro");
        paths.push("/docs/setup");
        assert_eq!(
            learn(&paths),
            ["/about", "/docs/intro", "/docs/setup", "/posts/{id}"]
        );
    }

    #[test]
    fn fixed_names_stay() {
        for name in [
            "api",
            "v1",
            "users",
            "me",
            "login",
            "graphql",
            "index.html",
            "2fa",
        ] {
            assert!(!looks_dynamic(name), "{name}");
        }
        for id in ["42", "2026-09-25", "deadbeef01", "a1b2c3d4e5f6a7b8c9d0"] {
            assert!(looks_dynamic(id), "{id}");
        }
    }

    proptest! {
        /// Every observed path fits one of the learned templates, and no numeric segment
        /// survives as a literal.
        #[test]
        fn every_path_fits_a_template(
            ids in proptest::collection::vec(1u64..10_000_000, 1..20),
            resources in proptest::collection::vec("[a-z]{2,8}", 1..4),
        ) {
            let paths: Vec<Vec<String>> = ids
                .iter()
                .enumerate()
                .map(|(n, id)| {
                    let resource = &resources[n % resources.len()];
                    vec![resource.clone(), id.to_string()]
                })
                .collect();
            let templates = Templates::learn(paths.iter().map(Vec::as_slice));
            for path in &paths {
                let template = templates.find(path);
                prop_assert!(template.is_some());
            }
            for template in &templates.templates {
                for segment in template {
                    if let Segment::Static(text) = segment {
                        prop_assert!(!text.chars().all(|c| c.is_ascii_digit()));
                    }
                }
            }
            prop_assert!(templates.templates.len() <= resources.len());
        }

        /// Rendered templates have unique parameter names.
        #[test]
        fn parameter_names_are_unique(shape in proptest::collection::vec(any::<bool>(), 0..8)) {
            let template: Vec<Segment> = shape
                .iter()
                .map(|param| if *param { Segment::Param } else { Segment::Static("items".into()) })
                .collect();
            let names = param_names(&template);
            let unique: BTreeSet<&String> = names.iter().collect();
            prop_assert_eq!(unique.len(), names.len());
        }
    }
}
