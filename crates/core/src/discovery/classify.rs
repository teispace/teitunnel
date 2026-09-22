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
            Self::Vite | Self::Next | Self::Astro | Self::Nuxt => 0,
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
        return if has("vite") {
            ServiceKind::Vite
        } else if has("next") {
            ServiceKind::Next
        } else if has("astro") {
            ServiceKind::Astro
        } else if has("nuxt") {
            ServiceKind::Nuxt
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
        return ServiceKind::Python;
    }
    if lower.starts_with("ruby") || matches!(lower.as_str(), "puma" | "rails" | "unicorn") {
        return ServiceKind::Ruby;
    }
    if lower.starts_with("php") || lower == "frankenphp" {
        return ServiceKind::Php;
    }
    if lower == "java" || has("gradle") || has(".jar") {
        return ServiceKind::Java;
    }
    if has("go-build") || (port >= 1024 && lower == "main") {
        return ServiceKind::Go;
    }
    ServiceKind::Other
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
            ("Python", "python3 manage.py runserver", ServiceKind::Python),
            ("uvicorn", "uvicorn app:app", ServiceKind::Python),
            ("ruby", "ruby bin/rails server", ServiceKind::Ruby),
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
}
