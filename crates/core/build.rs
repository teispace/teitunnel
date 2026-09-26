//! Generates typed constructors for the core's messages.
//!
//! The catalogs live in `locales/<language>.json` at the repository root, shared with
//! the desktop UI. Messages the core produces are under the top-level `core` key. For
//! each one this emits a function in `text::msg` that takes exactly the message's
//! placeholders, so a wrong key or a missing argument fails to compile:
//!
//! `"core": { "doctor": { "dnsMissing": { "title": "{hostname} has no DNS record" } } }`
//! becomes `msg::doctor::dns_missing::title(hostname: impl Display) -> Text`.
//!
//! Plurals (`name_one`, `name_other`, …) become one function taking `count: u64`
//! first. Every catalog is also embedded so the core can render text in the user's
//! language (menus, notifications, the CLI).

// A build script reports a bad catalog by failing the build.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use std::{
    collections::BTreeMap,
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};

const PLURAL_SUFFIXES: [&str; 6] = ["_zero", "_one", "_two", "_few", "_many", "_other"];

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
    "try", "type", "unsafe", "use", "where", "while", "yield",
];

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            panic!("message key `{name}` must be camelCase ASCII");
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if KEYWORDS.contains(&out.as_str()) {
        format!("r#{out}")
    } else {
        out
    }
}

/// `{name}` placeholders in order of first appearance.
fn placeholders(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric())
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_owned());
        }
        rest = &after[end + 1..];
    }
    out
}

fn plural_base(key: &str) -> Option<&str> {
    PLURAL_SUFFIXES.iter().find_map(|s| key.strip_suffix(s))
}

fn doc(text: &str) -> String {
    text.replace('\n', " ")
}

fn emit(out: &mut String, node: &Map<String, Value>, path: &str, depth: usize) {
    let indent = "    ".repeat(depth);
    // Leaves, grouping plural variants under their base name.
    let mut leaves: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for (key, value) in node {
        let Value::String(text) = value else { continue };
        // A platform's wording (`key@windows`) is picked at runtime; the base message
        // defines the function.
        if key.contains('@') {
            continue;
        }
        match plural_base(key) {
            Some(base) => {
                let entry = leaves
                    .entry(base.to_owned())
                    .or_insert_with(|| (String::new(), true));
                if key.ends_with("_other") {
                    entry.0.clone_from(text);
                }
            }
            None => {
                assert!(
                    !leaves.contains_key(key),
                    "`{path}.{key}` is both a message and a plural"
                );
                leaves.insert(key.clone(), (text.clone(), false));
            }
        }
    }
    for (name, (text, plural)) in &leaves {
        assert!(
            !plural || !text.is_empty(),
            "plural `{path}.{name}` needs an `_other` form"
        );
        assert!(
            !node.get(name).is_some_and(Value::is_object),
            "`{path}.{name}` is both a message and a group"
        );
        let args: Vec<String> = placeholders(text)
            .into_iter()
            .filter(|p| !(*plural && p == "count"))
            .collect();
        let mut params: Vec<String> = Vec::new();
        if *plural {
            params.push("count: u64".to_owned());
        }
        params.extend(
            args.iter()
                .map(|a| format!("{}: impl ::std::fmt::Display", snake(a))),
        );
        let _ = writeln!(out, "{indent}/// \"{}\"", doc(text));
        let _ = writeln!(out, "{indent}#[must_use]");
        if params.len() > 7 {
            let _ = writeln!(out, "{indent}#[allow(clippy::too_many_arguments)]");
        }
        let _ = writeln!(
            out,
            "{indent}pub fn {}({}) -> Text {{",
            snake(name),
            params.join(", ")
        );
        let _ = write!(out, "{indent}    Text::new(\"{path}.{name}\")");
        if *plural {
            let _ = write!(out, ".count(count)");
        }
        for arg in &args {
            let _ = write!(out, ".arg(\"{arg}\", {})", snake(arg));
        }
        let _ = writeln!(out, "\n{indent}}}");
    }
    for (key, value) in node {
        if let Value::Object(child) = value {
            let _ = writeln!(out, "{indent}/// `{path}.{key}`");
            let _ = writeln!(out, "{indent}pub mod {} {{", snake(key));
            let _ = writeln!(out, "{indent}    #[allow(unused_imports)]");
            let _ = writeln!(out, "{indent}    use crate::text::Text;");
            emit(out, child, &format!("{path}.{key}"), depth + 1);
            let _ = writeln!(out, "{indent}}}");
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    let locales = manifest.join("../../locales");
    println!("cargo::rerun-if-changed={}", locales.display());

    let read = |path: &Path| -> Value {
        let text =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
    };
    let english = read(&locales.join("en.json"));
    let core = english
        .get("core")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut messages =
        String::from("// Generated by build.rs from locales/en.json. Do not edit.\n");
    emit(&mut messages, &core, "core", 0);

    let mut catalogs: Vec<(String, PathBuf)> = fs::read_dir(&locales)
        .expect("locales directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .map(|path| {
            let lang = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("utf-8 file name")
                .to_owned();
            read(&path); // Fail the build on a malformed catalog.
            (lang, path.canonicalize().expect("catalog path"))
        })
        .collect();
    catalogs.sort();
    let mut embedded = String::from(
        "// Generated by build.rs. Do not edit.\n/// Every catalog, by language tag.\npub(crate) static CATALOGS: &[(&str, &str)] = &[\n",
    );
    for (lang, path) in &catalogs {
        let _ = writeln!(
            embedded,
            "    ({lang:?}, include_str!({:?})),",
            path.display().to_string()
        );
    }
    embedded.push_str("];\n");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets it"));
    fs::write(out.join("messages.rs"), messages).expect("write messages.rs");
    fs::write(out.join("catalogs.rs"), embedded).expect("write catalogs.rs");
}
