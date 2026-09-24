use serde::Serialize;

/// What a listening process appears to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ServiceKind {
    /// Vite dev server.
    Vite,
    /// Next.js.
    Next,
    /// Astro.
    Astro,
    /// Nuxt.
    Nuxt,
    /// Remix / React Router.
    Remix,
    /// SvelteKit (a Vite project with `svelte.config.js`).
    SvelteKit,
    /// Angular CLI (`ng serve`).
    Angular,
    /// webpack-dev-server (webpack, Create React App, Vue CLI).
    Webpack,
    /// Django.
    Django,
    /// Flask.
    Flask,
    /// FastAPI.
    FastApi,
    /// Ruby on Rails.
    Rails,
    /// Laravel (`artisan serve`).
    Laravel,
    /// Hugo.
    Hugo,
    /// Jekyll.
    Jekyll,
    /// Another Node.js / Bun / Deno server.
    Node,
    /// Python (Django, Flask, FastAPI/uvicorn, http.server…).
    Python,
    /// Ruby (Rails, Puma…).
    Ruby,
    /// PHP.
    Php,
    /// Java / JVM.
    Java,
    /// Go.
    Go,
    /// A port published by Docker, OrbStack or Colima.
    Docker,
    /// A database or cache (not HTTP).
    Database,
    /// A macOS system service, e.g. AirPlay Receiver on 5000/7000.
    System,
    /// Anything else.
    Other,
}

impl ServiceKind {
    /// Sort order in pickers: dev servers first, system services last.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Vite
            | Self::Next
            | Self::Astro
            | Self::Nuxt
            | Self::Remix
            | Self::SvelteKit
            | Self::Angular
            | Self::Webpack
            | Self::Django
            | Self::Flask
            | Self::FastApi
            | Self::Rails
            | Self::Laravel
            | Self::Hugo
            | Self::Jekyll => 0,
            Self::Node | Self::Python | Self::Ruby | Self::Php | Self::Java | Self::Go => 1,
            Self::Docker => 2,
            Self::Other => 3,
            Self::Database => 4,
            Self::System => 5,
        }
    }

    /// Suggested origin URL.
    pub(crate) fn origin(self, port: u16) -> String {
        match self {
            Self::Database => format!("tcp://localhost:{port}"),
            _ => format!("http://localhost:{port}"),
        }
    }
}

/// macOS daemons that listen on ports developers also use (AirPlay Receiver takes
/// 5000 and 7000 by default).
const SYSTEM_PROCESSES: &[&str] = &[
    "ControlCenter",
    "rapportd",
    "sharingd",
    "launchd",
    "mDNSResponder",
    "remoted",
    "identityservicesd",
    "AirPlayXPCHelper",
];

const DATABASE_PROCESSES: &[&str] = &[
    "postgres",
    "mysqld",
    "mariadbd",
    "redis-server",
    "mongod",
    "memcached",
    "valkey-server",
];

/// Classifies a process from its name and command line.
pub(crate) fn classify(name: &str, cmd: &[String], port: u16) -> ServiceKind {
    let lower = name.to_ascii_lowercase();
    let args = cmd.join(" ").to_ascii_lowercase();
    let has = |needle: &str| args.contains(needle);

    if SYSTEM_PROCESSES
        .iter()
        .any(|p| name.eq_ignore_ascii_case(p))
    {
        return ServiceKind::System;
    }
    if DATABASE_PROCESSES.iter().any(|p| lower.starts_with(p)) {
        return ServiceKind::Database;
    }
    if lower.contains("docker")
        || lower.contains("orbstack")
        || lower.contains("vpnkit")
        || lower.contains("colima")
        || lower == "limactl"
    {
        return ServiceKind::Docker;
    }
    if matches!(lower.as_str(), "node" | "bun" | "deno") || lower.starts_with("node") {
        return if has("@angular/cli") || has("ng serve") {
            ServiceKind::Angular
        } else if has("vite") {
            ServiceKind::Vite
        } else if has("next") {
            ServiceKind::Next
        } else if has("astro") {
            ServiceKind::Astro
        } else if has("nuxt") {
            ServiceKind::Nuxt
        } else if has("remix") || has("react-router") {
            ServiceKind::Remix
        } else if has("webpack") || has("react-scripts") || has("vue-cli-service") {
            ServiceKind::Webpack
        } else {
            ServiceKind::Node
        };
    }
    if lower.starts_with("python")
        || matches!(
            lower.as_str(),
            "uvicorn" | "gunicorn" | "hypercorn" | "granian"
        )
    {
        return if has("manage.py") || has("django") {
            ServiceKind::Django
        } else if has("flask") {
            ServiceKind::Flask
        } else if has("fastapi") {
            ServiceKind::FastApi
        } else {
            ServiceKind::Python
        };
    }
    if lower == "hugo" {
        return ServiceKind::Hugo;
    }
    if lower.starts_with("ruby")
        || lower == "jekyll"
        || matches!(lower.as_str(), "puma" | "rails" | "unicorn")
    {
        return if has("jekyll") || lower == "jekyll" {
            ServiceKind::Jekyll
        } else if has("rails") {
            ServiceKind::Rails
        } else {
            ServiceKind::Ruby
        };
    }
    if lower.starts_with("php") || lower == "frankenphp" {
        return if has("artisan") {
            ServiceKind::Laravel
        } else {
            ServiceKind::Php
        };
    }
    if lower == "java" || has("gradle") || has(".jar") {
        return ServiceKind::Java;
    }
    if has("go-build") || (port >= 1024 && lower == "main") {
        return ServiceKind::Go;
    }
    ServiceKind::Other
}

/// Frameworks built on Vite run as `vite dev`; their config file in the process's
/// directory says which one it is.
pub(crate) fn refine(kind: ServiceKind, cwd: Option<&std::path::Path>) -> ServiceKind {
    let Some(dir) = cwd.filter(|_| kind == ServiceKind::Vite) else {
        return kind;
    };
    let has = |names: &[&str]| names.iter().any(|name| dir.join(name).is_file());
    if has(&["svelte.config.js", "svelte.config.ts", "svelte.config.mjs"]) {
        ServiceKind::SvelteKit
    } else if has(&["astro.config.mjs", "astro.config.ts", "astro.config.js"]) {
        ServiceKind::Astro
    } else if has(&["nuxt.config.ts", "nuxt.config.js"]) {
        ServiceKind::Nuxt
    } else {
        kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn recognises_common_dev_servers() {
        let cases = [
            (
                "node",
                "node /app/node_modules/.bin/vite --port 5173",
                ServiceKind::Vite,
            ),
            (
                "node",
                "node /app/node_modules/next/dist/bin/next dev",
                ServiceKind::Next,
            ),
            ("node", "node server.js", ServiceKind::Node),
            ("bun", "bun run astro dev", ServiceKind::Astro),
            ("Python", "python3 -m http.server 8000", ServiceKind::Python),
            ("uvicorn", "uvicorn app:app", ServiceKind::Python),
            ("ruby", "puma 6.4.2 (tcp://0.0.0.0:3000)", ServiceKind::Ruby),
            ("php", "php -S localhost:8000", ServiceKind::Php),
            ("com.docker.backend", "", ServiceKind::Docker),
            ("OrbStack Helper", "", ServiceKind::Docker),
            ("postgres", "postgres -D /data", ServiceKind::Database),
            ("ControlCenter", "", ServiceKind::System),
            ("java", "java -jar app.jar", ServiceKind::Java),
            ("mystery", "", ServiceKind::Other),
        ];
        for (name, args, expected) in cases {
            assert_eq!(classify(name, &cmd(args), 3000), expected, "{name} {args}");
        }
    }

    #[test]
    fn databases_get_tcp_origins_and_rank_low() {
        assert_eq!(ServiceKind::Database.origin(5432), "tcp://localhost:5432");
        assert_eq!(ServiceKind::Vite.origin(5173), "http://localhost:5173");
        assert!(ServiceKind::Vite.rank() < ServiceKind::System.rank());
    }

    #[test]
    fn recognises_frameworks_from_the_command_line() {
        let cases = [
            (
                "python3.13",
                "python manage.py runserver",
                ServiceKind::Django,
            ),
            ("python3", "python -m flask run", ServiceKind::Flask),
            ("python3", "fastapi dev main.py", ServiceKind::FastApi),
            ("uvicorn", "uvicorn app:app", ServiceKind::Python),
            ("ruby", "ruby bin/rails server", ServiceKind::Rails),
            (
                "ruby",
                "ruby /usr/local/bin/jekyll serve",
                ServiceKind::Jekyll,
            ),
            ("php", "php artisan serve", ServiceKind::Laravel),
            ("hugo", "hugo server", ServiceKind::Hugo),
            (
                "node",
                "node node_modules/.bin/remix vite:dev",
                ServiceKind::Vite,
            ),
            ("node", "node react-router dev", ServiceKind::Remix),
            (
                "node",
                "node /app/node_modules/@angular/cli/bin/ng serve",
                ServiceKind::Angular,
            ),
            (
                "node",
                "node /app/node_modules/.bin/webpack serve --mode development",
                ServiceKind::Webpack,
            ),
            (
                "node",
                "node /app/node_modules/react-scripts/scripts/start.js",
                ServiceKind::Webpack,
            ),
        ];
        for (name, line, kind) in cases {
            assert_eq!(classify(name, &cmd(line), 3000), kind, "{line}");
        }
    }

    #[test]
    fn vite_projects_say_which_framework_they_are() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            refine(ServiceKind::Vite, Some(dir.path())),
            ServiceKind::Vite
        );
        std::fs::write(dir.path().join("svelte.config.js"), "").unwrap();
        assert_eq!(
            refine(ServiceKind::Vite, Some(dir.path())),
            ServiceKind::SvelteKit
        );
        assert_eq!(
            refine(ServiceKind::Next, Some(dir.path())),
            ServiceKind::Next
        );
        assert_eq!(refine(ServiceKind::Vite, None), ServiceKind::Vite);
        let astro = tempfile::tempdir().unwrap();
        std::fs::write(astro.path().join("astro.config.mjs"), "").unwrap();
        assert_eq!(
            refine(ServiceKind::Vite, Some(astro.path())),
            ServiceKind::Astro
        );
    }
}
