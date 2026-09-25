//! Protection: password page, secret link, basic auth, IP and user-agent rules, bypass.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, Method, Request, StatusCode};
use lens::{
    AgentPreset, BasicAuth, Filter, GateOutcome, Gates, PasswordGate, PathPattern, Responder,
    SecretLink, Upstream,
};

use crate::support::*;

/// An origin that reports the Cookie and Authorization headers it received.
async fn reporting_origin() -> Origin {
    origin(|request: Request<hyper::body::Incoming>| async move {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .map_or("-", |v| v.to_str().unwrap_or("?"))
                .to_owned()
        };
        let text = format!(
            "cookie={} authorization={}",
            header("cookie"),
            header("authorization")
        );
        text_response(200, text)
    })
    .await
}

fn set_cookie(headers: &HeaderMap) -> String {
    let value = headers["set-cookie"].to_str().unwrap();
    value.split(';').next().unwrap().to_owned()
}

fn login(password: &str, next: &str) -> Request<lens::LensBody> {
    let form = format!(
        "password={}&next={}",
        percent_encode(password),
        percent_encode(next)
    );
    request(Method::POST, lens::LOGIN_PATH)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body(form))
        .unwrap()
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn password_page_signs_in_with_a_cookie() {
    let origin = reporting_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates = Gates {
            password: Some(PasswordGate::new("open sesame").unwrap()),
            secure_cookie: false,
            bypass: vec![PathPattern::parse("/webhooks/*").unwrap()],
            ..Gates::default()
        };
    })
    .await;

    let page = get(tap.addr, "/dashboard?tab=1").await;
    assert_eq!(page.status, StatusCode::UNAUTHORIZED);
    assert!(page.text().contains("name=\"password\""));
    assert!(page.text().contains("value=\"/dashboard?tab=1\""));
    assert!(page.headers.contains_key("content-security-policy"));

    let wrong = fetch(tap.addr, login("nope", "/dashboard")).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(wrong.text().contains("isn&#39;t right"));

    let right = fetch(tap.addr, login("open sesame", "/dashboard?tab=1")).await;
    assert_eq!(right.status, StatusCode::SEE_OTHER);
    assert_eq!(right.headers["location"], "/dashboard?tab=1");
    let raw_cookie = right.headers["set-cookie"].to_str().unwrap().to_owned();
    assert!(raw_cookie.contains("HttpOnly") && raw_cookie.contains("SameSite=Lax"));
    let cookie = set_cookie(&right.headers);

    let inside = fetch(
        tap.addr,
        request(Method::GET, "/dashboard")
            .header("cookie", format!("theme=dark; {cookie}"))
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(inside.status, StatusCode::OK);
    assert_eq!(
        inside.text(),
        "cookie=theme=dark authorization=-",
        "Lens's cookie never reaches the origin"
    );

    // A forged cookie doesn't work.
    let forged = fetch(
        tap.addr,
        request(Method::GET, "/")
            .header("cookie", format!("{cookie}x"))
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);

    // Webhooks bypass the password.
    assert_eq!(
        get(tap.addr, "/webhooks/github").await.status,
        StatusCode::OK
    );

    // The password never lands in the capture.
    let attempt = next_exchange(
        &lens,
        Filter {
            methods: vec!["POST".into()],
            ..Filter::default()
        },
    )
    .await;
    assert!(attempt.request.body.data.is_empty());
    assert!(attempt.request.body.size > 0);
    assert!(matches!(
        attempt.responder,
        Responder::Gate {
            reason: GateOutcome::PasswordWrong
        }
    ));

    // Changing the password signs everyone out.
    lens.update_tap(&tap.id, |config| {
        config.gates.password = Some(PasswordGate::new("new one").unwrap());
    })
    .unwrap();
    let after = fetch(
        tap.addr,
        request(Method::GET, "/")
            .header("cookie", cookie)
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn password_attempts_are_rate_limited() {
    let origin = reporting_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.password = Some(PasswordGate::new("right").unwrap());
    })
    .await;
    for _ in 0..10 {
        assert_eq!(
            fetch(tap.addr, login("wrong", "/")).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    let limited = fetch(tap.addr, login("right", "/")).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.contains_key("retry-after"));
    assert!(lens.metrics(&tap.id).unwrap().blocked >= 11);
}

#[tokio::test]
async fn secret_link_sets_a_cookie_and_hides_the_key() {
    let origin = reporting_origin().await;
    let link = SecretLink::generate().unwrap();
    let token = link.token().expose().clone();
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.secret_link = Some(link);
        config.gates.secure_cookie = false;
    })
    .await;
    let without = get(tap.addr, "/report").await;
    assert_eq!(without.status, StatusCode::FORBIDDEN);
    let wrong = get(tap.addr, "/report?key=wrong-key-value-123").await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);

    let with = get(tap.addr, &format!("/report?a=1&key={token}&b=2")).await;
    assert_eq!(with.status, StatusCode::SEE_OTHER);
    assert_eq!(with.headers["location"], "/report?a=1&b=2");
    let cookie = set_cookie(&with.headers);
    let inside = fetch(
        tap.addr,
        request(Method::GET, "/report?a=1&b=2")
            .header("cookie", cookie)
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(inside.status, StatusCode::OK);
}

#[tokio::test]
async fn basic_auth_challenges_and_is_stripped() {
    let origin = reporting_origin().await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.basic = Some(BasicAuth::new("ada", "lovelace").unwrap());
    })
    .await;
    let challenge = get(tap.addr, "/").await;
    assert_eq!(challenge.status, StatusCode::UNAUTHORIZED);
    assert!(
        challenge.headers["www-authenticate"]
            .to_str()
            .unwrap()
            .starts_with("Basic")
    );
    let credentials = format!("Basic {}", STANDARD.encode("ada:lovelace"));
    let ok = fetch(
        tap.addr,
        request(Method::GET, "/")
            .header("authorization", credentials)
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.text(), "cookie=- authorization=-");
    let wrong = format!("Basic {}", STANDARD.encode("ada:babbage"));
    let refused = fetch(
        tap.addr,
        request(Method::GET, "/")
            .header("authorization", wrong)
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ip_rules_trust_cf_connecting_ip_only_when_configured() {
    let origin = reporting_origin().await;
    let with_ip = |ip: &'static str| {
        request(Method::GET, "/")
            .header("cf-connecting-ip", ip)
            .body(lens::empty())
            .unwrap()
    };
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.trust_cf_connecting_ip = true;
        config.gates.ip_allow = vec!["198.51.100.0/24".parse().unwrap()];
        config.gates.ip_deny = vec!["198.51.100.66/32".parse().unwrap()];
    })
    .await;
    assert_eq!(
        fetch(tap.addr, with_ip("198.51.100.10")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        fetch(tap.addr, with_ip("198.51.100.66")).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fetch(tap.addr, with_ip("203.0.113.1")).await.status,
        StatusCode::FORBIDDEN
    );
    let denied = next_exchange(
        &lens,
        Filter {
            statuses: vec![403],
            ..Filter::default()
        },
    )
    .await;
    assert!(matches!(
        denied.responder,
        Responder::Gate {
            reason: GateOutcome::IpDenied
        }
    ));

    // Without trust, the header is ignored: the peer (127.0.0.1) decides.
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.ip_deny = vec!["127.0.0.0/8".parse().unwrap()];
    })
    .await;
    assert_eq!(
        fetch(tap.addr, with_ip("198.51.100.10")).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn user_agent_rules_block_crawlers_even_on_bypassed_paths() {
    let origin = reporting_origin().await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.agent_presets = vec![AgentPreset::AiCrawlers];
        config.gates.bypass = vec![PathPattern::parse("/webhooks/*").unwrap()];
    })
    .await;
    let as_agent = |ua: &'static str, path: &str| {
        request(Method::GET, path)
            .header("user-agent", ua)
            .body(lens::empty())
            .unwrap()
    };
    let bot = "Mozilla/5.0 (compatible; GPTBot/1.2; +https://openai.com/gptbot)";
    assert_eq!(
        fetch(tap.addr, as_agent(bot, "/")).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fetch(tap.addr, as_agent(bot, "/webhooks/x")).await.status,
        StatusCode::FORBIDDEN
    );
    let browser =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 Safari/605.1.15";
    assert_eq!(
        fetch(tap.addr, as_agent(browser, "/")).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn gated_pages_are_not_injectable_with_markup() {
    let origin = reporting_origin().await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.password = Some(PasswordGate::new("pw").unwrap());
    })
    .await;
    let page = get(tap.addr, "/%22%3E%3Cscript%3Ealert(1)%3C/script%3E").await;
    let html = page.text();
    assert!(!html.contains("<script>alert"), "{html}");
    // Open redirects are refused after signing in.
    let reply = fetch(tap.addr, login("pw", "//evil.example")).await;
    assert_eq!(reply.headers["location"], "/");
    assert!(reply.body.is_empty());
}
