//! What a `User-Agent` says it is: a browser family, or a bot and its kind.
//!
//! The inspector sees every request's `User-Agent`, but, unlike Cloudflare's verified
//! bots, can't check the claim: a scraper can call itself Googlebot. Names follow
//! Cloudflare's own (`userAgentBrowser` and `verifiedBotCategory`), so the local and the
//! edge breakdowns read the same.

/// What a `User-Agent` claims to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    /// A person's browser: its family (`Chrome`, `Safari`…).
    Browser(&'static str),
    /// A bot of this kind (`Search Engine Crawler`, `AI Crawler`…).
    Bot(&'static str),
}

/// Bots by a distinctive part of their `User-Agent` (lowercase), most specific first.
const BOTS: &[(&str, &str)] = &[
    // AI: assistants fetching a page for a person, search indexes, training crawlers.
    ("chatgpt-user", "AI Assistant"),
    ("claude-user", "AI Assistant"),
    ("perplexity-user", "AI Assistant"),
    ("mistralai-user", "AI Assistant"),
    ("oai-searchbot", "AI Search"),
    ("claude-searchbot", "AI Search"),
    ("perplexitybot", "AI Search"),
    ("gptbot", "AI Crawler"),
    ("claudebot", "AI Crawler"),
    ("anthropic-ai", "AI Crawler"),
    ("ccbot", "AI Crawler"),
    ("bytespider", "AI Crawler"),
    ("google-extended", "AI Crawler"),
    ("meta-externalagent", "AI Crawler"),
    ("amazonbot", "AI Crawler"),
    ("applebot-extended", "AI Crawler"),
    ("cohere-ai", "AI Crawler"),
    ("diffbot", "AI Crawler"),
    // Link previews in chats and social networks.
    ("facebookexternalhit", "Page Preview"),
    ("twitterbot", "Page Preview"),
    ("slackbot", "Page Preview"),
    ("discordbot", "Page Preview"),
    ("linkedinbot", "Page Preview"),
    ("telegrambot", "Page Preview"),
    ("whatsapp", "Page Preview"),
    ("skypeuripreview", "Page Preview"),
    ("iframely", "Page Preview"),
    // Search engines.
    ("googlebot", "Search Engine Crawler"),
    ("bingbot", "Search Engine Crawler"),
    ("duckduckbot", "Search Engine Crawler"),
    ("yandexbot", "Search Engine Crawler"),
    ("baiduspider", "Search Engine Crawler"),
    ("applebot", "Search Engine Crawler"),
    ("slurp", "Search Engine Crawler"),
    // Monitoring, Teitunnel's own checks included.
    ("(uptime check)", "Monitoring & Analytics"),
    ("uptimerobot", "Monitoring & Analytics"),
    ("pingdom", "Monitoring & Analytics"),
    ("statuscake", "Monitoring & Analytics"),
    ("betteruptime", "Monitoring & Analytics"),
    ("better stack", "Monitoring & Analytics"),
    ("checkly", "Monitoring & Analytics"),
    ("datadog", "Monitoring & Analytics"),
    ("newrelic", "Monitoring & Analytics"),
    ("site24x7", "Monitoring & Analytics"),
    ("lighthouse", "Monitoring & Analytics"),
    // Webhook senders.
    ("github-hookshot", "Webhooks"),
    ("stripe/", "Webhooks"),
    ("shopify", "Webhooks"),
    ("gitlab", "Webhooks"),
    ("bitbucket-webhooks", "Webhooks"),
    ("twilio", "Webhooks"),
    ("sendgrid", "Webhooks"),
    ("mailgun", "Webhooks"),
    ("svix", "Webhooks"),
    ("paypal", "Webhooks"),
    ("linear-webhook", "Webhooks"),
    ("vercel", "Webhooks"),
    // Feeds.
    ("feedly", "Feed Fetcher"),
    ("feedbin", "Feed Fetcher"),
    ("newsblur", "Feed Fetcher"),
    ("inoreader", "Feed Fetcher"),
    // Security scanners.
    ("censysinspect", "Security"),
    ("expanse", "Security"),
    ("zgrab", "Security"),
    ("nmap", "Security"),
    ("nuclei", "Security"),
    ("masscan", "Security"),
];

/// Command-line and code HTTP clients: not browsers, not announced bots.
const TOOLS: &[&str] = &[
    "curl/",
    "wget/",
    "httpie/",
    "python-requests/",
    "python-urllib/",
    "python-httpx/",
    "aiohttp/",
    "go-http-client/",
    "node-fetch",
    "undici",
    "axios/",
    "okhttp/",
    "java/",
    "apache-httpclient/",
    "ruby",
    "faraday",
    "guzzlehttp/",
    "reqwest",
    "postmanruntime/",
    "insomnia/",
    "bruno",
    "deno/",
    "bun/",
    "dart:io",
    "libwww-perl",
    "powershell",
];

/// What `user_agent` claims to be; `None` for an empty header or one that says nothing
/// recognisable.
pub fn classify(user_agent: &str) -> Option<Agent> {
    let ua = user_agent.trim();
    if ua.is_empty() {
        return None;
    }
    let lower = ua.to_ascii_lowercase();
    if let Some((_, kind)) = BOTS.iter().find(|(needle, _)| lower.contains(needle)) {
        return Some(Agent::Bot(kind));
    }
    if TOOLS.iter().any(|tool| lower.starts_with(tool)) {
        return Some(Agent::Bot("Tools & Scripts"));
    }
    if announces_bot(&lower) {
        return Some(Agent::Bot("Other Bot"));
    }
    browser(&lower).map(Agent::Browser)
}

/// Whether a product name ends in `bot`, `crawler` or `spider` (`SomeBot/1.0`,
/// `(compatible; somecrawler)`), but not a device name that merely contains it
/// (`CUBOT X30`).
fn announces_bot(lower: &str) -> bool {
    ["bot", "crawler", "spider"].iter().any(|word| {
        lower.match_indices(word).any(|(at, _)| {
            matches!(
                lower[at + word.len()..].chars().next(),
                None | Some('/' | ';' | ')' | '-' | '+')
            )
        })
    })
}

/// The browser family, checked in the order that tells them apart: every Chromium
/// browser also says `Chrome/` and `Safari/`, and Chrome also says `Safari/`.
fn browser(lower: &str) -> Option<&'static str> {
    const FAMILIES: &[(&str, &str)] = &[
        ("edg/", "Edge"),
        ("edga/", "Edge"),
        ("edgios/", "Edge"),
        ("opr/", "Opera"),
        ("opera", "Opera"),
        ("vivaldi/", "Vivaldi"),
        ("yabrowser/", "Yandex Browser"),
        ("samsungbrowser/", "Samsung Internet"),
        ("ucbrowser/", "UC Browser"),
        ("firefox/", "Firefox"),
        ("fxios/", "Firefox"),
        ("crios/", "Chrome"),
        ("chromium/", "Chromium"),
        ("chrome/", "Chrome"),
        ("safari/", "Safari"),
        ("msie ", "Internet Explorer"),
        ("trident/", "Internet Explorer"),
    ];
    if let Some((_, family)) = FAMILIES.iter().find(|(needle, _)| lower.contains(needle)) {
        return Some(family);
    }
    // In-app browsers on iOS (WKWebView) say Mobile/… without Safari/.
    (lower.starts_with("mozilla/") && lower.contains("applewebkit/")).then_some("Safari")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_browsers_apart() {
        let cases = [
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15",
                "Safari",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
                "Chrome",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0",
                "Edge",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0",
                "Firefox",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/140.0 Mobile/15E148 Safari/604.1",
                "Chrome",
            ),
            (
                "Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/28.0 Chrome/130.0 Mobile Safari/537.36",
                "Samsung Internet",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
                "Safari",
            ),
        ];
        for (ua, family) in cases {
            assert_eq!(classify(ua), Some(Agent::Browser(family)), "{ua}");
        }
    }

    #[test]
    fn names_bots_by_kind() {
        let cases = [
            (
                "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
                "Search Engine Crawler",
            ),
            (
                "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; GPTBot/1.2; +https://openai.com/gptbot)",
                "AI Crawler",
            ),
            (
                "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko); compatible; ChatGPT-User/1.0",
                "AI Assistant",
            ),
            (
                "Mozilla/5.0 (compatible; Applebot-Extended/0.1)",
                "AI Crawler",
            ),
            (
                "Slackbot-LinkExpanding 1.0 (+https://api.slack.com/robots)",
                "Page Preview",
            ),
            ("GitHub-Hookshot/a1b2c3", "Webhooks"),
            ("Stripe/1.0 (+https://stripe.com/docs/webhooks)", "Webhooks"),
            ("Teitunnel/0.2.0 (uptime check)", "Monitoring & Analytics"),
            ("curl/8.7.1", "Tools & Scripts"),
            ("python-requests/2.32.3", "Tools & Scripts"),
            ("SomeCrawler/1.0", "Other Bot"),
        ];
        for (ua, kind) in cases {
            assert_eq!(classify(ua), Some(Agent::Bot(kind)), "{ua}");
        }
        assert_eq!(classify(""), None);
        assert_eq!(classify("  "), None);
        assert_eq!(classify("MyApp/1.0"), None);
        assert_eq!(
            classify("Mozilla/5.0 (compatible; somecrawler)"),
            Some(Agent::Bot("Other Bot"))
        );
        assert_eq!(
            classify(
                "Mozilla/5.0 (Linux; Android 12; CUBOT X30) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36"
            ),
            Some(Agent::Browser("Chrome")),
            "a phone named like a bot"
        );
    }
}
