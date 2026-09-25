//! The "Permissions and scopes" reference page
//! (`apps/web/content/docs/reference/permissions.mdx`), rendered from the permissions
//! Teitunnel asks for, and checks that its table covers all of them.
//!
//! `UPDATE_DOCS=1 cargo test -p teitunnel-core --test permissions_doc` rewrites the page;
//! without `UPDATE_DOCS` the test fails when the page is out of date.

use std::{collections::BTreeSet, fmt::Write as _};

use teitunnel_core::accounts::{
    capabilities::{PERMISSION_USES, PermissionUse, token_template_permissions},
    oauth::requested_scopes,
    token_template_url,
};

const PAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/web/content/docs/reference/permissions.mdx"
);

const LOCALE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales/en.json");

const REGENERATE: &str = "UPDATE_DOCS=1 cargo test -p teitunnel-core --test permissions_doc";

/// How many scopes at the start of the list the OAuth client requires (oauth.rs).
const REQUIRED_SCOPES: usize = 4;

#[test]
fn the_permissions_page_is_up_to_date() {
    let page = render();
    if std::env::var_os("UPDATE_DOCS").is_some() {
        std::fs::write(PAGE, &page).unwrap();
        return;
    }
    let current = std::fs::read_to_string(PAGE).unwrap_or_default();
    assert!(
        current == page,
        "Permissions reference is out of date: run `{REGENERATE}`"
    );
}

#[test]
fn every_token_key_is_documented_once() {
    for (key, _) in token_template_permissions() {
        let rows = PERMISSION_USES
            .iter()
            .filter(|p| p.token_key == Some(*key))
            .count();
        assert_eq!(rows, 1, "token key `{key}` is documented {rows} times");
    }
    for use_ in PERMISSION_USES {
        if let Some(key) = use_.token_key {
            assert!(
                token_template_permissions().iter().any(|(k, _)| *k == key),
                "`{key}` isn't in the token link"
            );
        }
    }
}

#[test]
fn every_scope_is_documented_once() {
    let scopes = requested_scopes();
    for scope in scopes {
        let rows = PERMISSION_USES
            .iter()
            .filter(|p| p.scopes.contains(scope))
            .count();
        assert_eq!(rows, 1, "scope `{scope}` is documented {rows} times");
    }
    for use_ in PERMISSION_USES {
        for scope in use_.scopes {
            assert!(
                scopes.contains(scope),
                "`{scope}` isn't requested at sign-in"
            );
        }
    }
    // What routes need is what the OAuth client requires.
    let required: BTreeSet<_> = PERMISSION_USES
        .iter()
        .filter(|p| p.required)
        .flat_map(|p| p.scopes.iter().copied())
        .collect();
    let client: BTreeSet<_> = scopes[..REQUIRED_SCOPES].iter().copied().collect();
    assert_eq!(required, client);
}

#[test]
fn names_match_the_app_and_the_token_link() {
    let locale: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(LOCALE).unwrap()).unwrap();
    for use_ in PERMISSION_USES {
        if let Some(key) = use_.fix_key {
            let text = locale["permissionFix"][key].as_str();
            assert_eq!(text, use_.name, "permissionFix.{key} in locales/en.json");
        }
        // "… · Edit" in the dashboard is `edit` in the link, "… · Read" is `read`.
        if let (Some(name), Some(key)) = (use_.name, use_.token_key) {
            let kind = token_template_permissions()
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, kind)| *kind)
                .unwrap();
            let level = name.rsplit(" · ").next().unwrap().to_lowercase();
            assert_eq!(level, kind, "{name} is `{key}` ({kind})");
        }
        // Plain Markdown: nothing MDX reads as JSX or an expression, nor a table pipe.
        let text = format!("{}{}", use_.name.unwrap_or_default(), use_.features);
        assert!(!text.contains(['<', '>', '{', '}', '|']), "{text}");
    }
}

fn render() -> String {
    let mut page = String::from(
        r#"---
title: Permissions and scopes
description: "Every Cloudflare permission and OAuth scope Teitunnel asks for, whether routes need it, and which features use it."
---
"#,
    );
    let _ = write!(
        page,
        "\n{{/* Generated from crates/core/src/accounts (capabilities.rs, template.rs, oauth.rs). \
         Edit it there, then run: {REGENERATE} */}}\n\n"
    );
    page.push_str(
        "Teitunnel connects to a Cloudflare account with an API token or by signing in with \
         Cloudflare. Either way it asks for the permissions below: the required ones are what \
         routes need; the optional ones are only used by the feature next to them, and \
         everything else keeps working without them. When a feature needs one the account's \
         credential lacks, it says which to add and checks again when you come back \
         ([Accounts and permissions](/docs/guides/accounts/#when-a-permission-is-missing)).\n\n\
         **Settings ▸ Accounts ▸ Permissions** shows what the connected credential can do. \
         The permissions with *Yes* under Checked are tested there without changing anything; the \
         others show up when Cloudflare refuses a change.\n\n",
    );
    section(
        &mut page,
        "Required",
        PERMISSION_USES.iter().filter(|p| p.required),
    );
    section(
        &mut page,
        "Optional",
        PERMISSION_USES
            .iter()
            .filter(|p| !p.required && p.name.is_some()),
    );
    page.push_str(
        "## Only when signing in with Cloudflare\n\n\
         A sign-in also asks for these scopes, which have no permission of their own in an API \
         token.\n\n\
         | OAuth scope | Used for |\n|---|---|\n",
    );
    for use_ in PERMISSION_USES.iter().filter(|p| p.name.is_none()) {
        let _ = writeln!(page, "| {} | {}. |", scopes(use_), use_.features);
    }
    page.push_str(
        "\nEvery optional scope can be declined on Cloudflare's consent screen; the feature \
         that needs it then asks for it again.\n\n\
         ## The token link\n\n\
         **Open Cloudflare** in **Settings ▸ Accounts ▸ Connect an Account** opens the dashboard's *Create API token* page with \
         these permissions selected, for all your accounts and domains:\n\n\
         ```text\n",
    );
    page.push_str(&token_template_url());
    page.push_str(
        "\n```\n\n\
         Load balancing isn't in the link, because it's a paid add-on: add its two permissions \
         to the token if you use it. Cloudflare ignores keys it doesn't know without saying so, \
         so if **Cloudflare Tunnel · Edit** is missing from the page it opens, add it by hand.\n",
    );
    page
}

fn section<'a>(page: &mut String, title: &str, uses: impl Iterator<Item = &'a PermissionUse>) {
    let _ = write!(
        page,
        "## {title}\n\n\
         | Permission | Token link key | OAuth scope | Used for | Checked |\n\
         |---|---|---|---|---|\n"
    );
    for use_ in uses {
        let key = use_
            .token_key
            .map_or_else(|| "not in the link".to_owned(), |k| format!("`{k}`"));
        let _ = writeln!(
            page,
            "| {} | {key} | {} | {}. | {} |",
            use_.name.unwrap_or_default(),
            scopes(use_),
            use_.features,
            if use_.probed { "Yes" } else { "No" },
        );
    }
    page.push('\n');
}

fn scopes(use_: &PermissionUse) -> String {
    let list: Vec<_> = use_.scopes.iter().map(|s| format!("`{s}`")).collect();
    list.join(", ")
}
