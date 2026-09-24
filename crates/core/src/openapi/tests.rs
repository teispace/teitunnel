use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, Version};
use lens::{
    BodyRecord, ClientInfo, ExchangeId, ExchangeState, RequestRecord, ResponseRecord, TapId,
    Timings,
};
use proptest::prelude::*;

use super::*;

fn exchange(
    method: &str,
    path: &str,
    request: Option<(&str, &str)>,
    status: u16,
    response: Option<(&str, &str)>,
    headers: &[(&'static str, &'static str)],
) -> Exchange {
    let mut request_headers = HeaderMap::new();
    request_headers.insert("host", HeaderValue::from_static("api.example.com"));
    request_headers.insert("user-agent", HeaderValue::from_static("curl/8"));
    for (name, value) in headers {
        request_headers.insert(*name, HeaderValue::from_static(value));
    }
    let request_body = match request {
        Some((media, body)) => {
            request_headers.insert("content-type", media.parse().unwrap());
            BodyRecord::full(Bytes::from(body.to_owned()))
        }
        None => BodyRecord::empty(),
    };
    let mut response_headers = HeaderMap::new();
    let response_body = match response {
        Some((media, body)) => {
            response_headers.insert("content-type", media.parse().unwrap());
            BodyRecord::full(Bytes::from(body.to_owned()))
        }
        None => BodyRecord::empty(),
    };
    Exchange {
        id: ExchangeId::from(uuid::Uuid::now_v7()),
        seq: 1,
        tap: TapId::new("t1").unwrap(),
        kind: ExchangeKind::Http,
        state: ExchangeState::Complete,
        started_at_ms: 1,
        timings: Timings::default(),
        client: ClientInfo {
            ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
            peer: SocketAddr::from(([127, 0, 0, 1], 5000)),
            cf_ray: None,
            country: None,
        },
        request: RequestRecord {
            method: method.parse::<Method>().unwrap(),
            uri: path.parse::<Uri>().unwrap(),
            scheme: "https".into(),
            host: "api.example.com".into(),
            version: Version::HTTP_11,
            headers: request_headers,
            body: request_body,
        },
        response: Some(ResponseRecord {
            status: StatusCode::from_u16(status).unwrap(),
            version: Version::HTTP_11,
            headers: response_headers,
            body: response_body,
        }),
        responder: Responder::Upstream,
        error: None,
        stream: None,
        replay_of: None,
        fault: None,
    }
}

const JSON: &str = "application/json";

#[test]
fn describes_an_api_from_its_traffic() {
    let exchanges = vec![
        exchange(
            "GET",
            "/users/123?expand=orders",
            None,
            200,
            Some((JSON, r#"{"id":123,"email":"a@b.co","tags":["x"]}"#)),
            &[("authorization", "Bearer masked"), ("x-api-version", "2")],
        ),
        exchange(
            "GET",
            "/users/456",
            None,
            200,
            Some((
                JSON,
                r#"{"id":456,"email":"c@d.io","tags":[],"nickname":null}"#,
            )),
            &[("authorization", "Bearer masked"), ("x-api-version", "2")],
        ),
        exchange(
            "GET",
            "/users/789",
            None,
            404,
            Some((JSON, r#"{"error":"not found"}"#)),
            &[],
        ),
        exchange(
            "POST",
            "/users",
            Some((JSON, r#"{"email":"e@f.dev","admin":false}"#)),
            201,
            Some((JSON, r#"{"id":1}"#)),
            &[],
        ),
        exchange(
            "POST",
            "/login",
            Some((
                "application/x-www-form-urlencoded",
                "user=a&password=%E2%80%A2",
            )),
            302,
            None,
            &[],
        ),
        // Not the API: a page, a script, a preflight, a paused answer.
        exchange("GET", "/", None, 200, Some(("text/html", "<p>hi</p>")), &[]),
        exchange(
            "GET",
            "/app.js",
            None,
            200,
            Some(("text/javascript", "x")),
            &[],
        ),
        exchange("OPTIONS", "/users", None, 204, None, &[]),
    ];
    let mut paused = exchange("GET", "/users/1", None, 503, None, &[]);
    paused.responder = Responder::Paused;
    let mut exchanges = exchanges;
    exchanges.push(paused);

    let (document, summary) = infer(&exchanges, &Options::default());
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["info"]["title"], "api.example.com API");
    assert_eq!(document["servers"][0]["url"], "https://api.example.com");
    assert_eq!(summary.requests, 5);
    assert_eq!(summary.skipped, 4);
    assert_eq!(summary.hosts, ["api.example.com"]);

    let paths = document["paths"].as_object().unwrap();
    let mut names: Vec<&String> = paths.keys().collect();
    names.sort();
    assert_eq!(names, ["/login", "/users", "/users/{id}"]);

    let get = &document["paths"]["/users/{id}"]["get"];
    let parameters = get["parameters"].as_array().unwrap();
    assert_eq!(
        parameters[0],
        json!({"name": "id", "in": "path", "required": true, "schema": {"type": "integer"}})
    );
    assert!(
        parameters
            .iter()
            .any(|p| p["name"] == "expand" && p["in"] == "query" && p.get("required").is_none())
    );
    assert!(
        parameters
            .iter()
            .any(|p| p["name"] == "x-api-version" && p["in"] == "header")
    );
    assert!(
        !parameters
            .iter()
            .any(|p| p["name"] == "user-agent" || p["name"] == "authorization")
    );
    let ok = &get["responses"]["200"]["content"][JSON]["schema"];
    assert_eq!(ok["properties"]["email"]["format"], "email");
    assert_eq!(ok["required"], json!(["email", "id", "tags"]));
    assert_eq!(ok["properties"]["nickname"]["type"], "null");
    assert_eq!(get["responses"]["404"]["description"], "Not Found");
    assert_eq!(get["security"], json!([{"bearerAuth": []}]));
    assert_eq!(
        document["components"]["securitySchemes"]["bearerAuth"],
        json!({"type": "http", "scheme": "bearer"})
    );

    let post = &document["paths"]["/users"]["post"];
    assert_eq!(post["requestBody"]["required"], true);
    assert_eq!(
        post["requestBody"]["content"][JSON]["schema"]["properties"]["admin"]["type"],
        "boolean"
    );
    let login = &document["paths"]["/login"]["post"]["requestBody"]["content"]["application/x-www-form-urlencoded"]
        ["schema"];
    assert_eq!(
        login["properties"],
        json!({"password": {"type": "string"}, "user": {"type": "string"}})
    );
    // No observed value is copied into the document.
    let text = document.to_string();
    for value in ["a@b.co", "c@d.io", "not found", "123"] {
        assert!(!text.contains(value), "{value} leaked into the document");
    }
    // YAML too.
    let yaml = render(&document, true).unwrap();
    assert!(
        yaml.contains("openapi:") && yaml.contains("3.1.0"),
        "{yaml}"
    );
}

#[test]
fn filters_by_host_and_handles_nothing() {
    let (document, summary) = infer(
        &[exchange("GET", "/a", None, 200, Some((JSON, "{}")), &[])],
        &Options {
            host: Some("other.example.com".into()),
            title: None,
        },
    );
    assert_eq!(summary.requests, 0);
    assert_eq!(document["paths"], json!({}));
    assert_eq!(document["info"]["title"], "Observed API");
}

#[test]
fn truncated_bodies_keep_the_media_type_only() {
    let mut big = exchange("POST", "/upload", Some((JSON, r#"{"a":"#)), 200, None, &[]);
    big.request.body.truncated = true;
    let (document, _) = infer(&[big], &Options::default());
    assert_eq!(
        document["paths"]["/upload"]["post"]["requestBody"]["content"][JSON]["schema"],
        json!({})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn describes_what_the_inspector_captured() {
    use crate::inspect::{
        Inspector, TapScope, TapSpec,
        tests::{origin, send},
    };
    let origin = origin().await;
    let inspector = Inspector::new(None, None, "app");
    let tap = inspector
        .start(TapSpec::new(
            TapScope::QuickShare {
                share_id: "qs-1".into(),
            },
            "demo",
            &origin,
        ))
        .await
        .unwrap();
    for path in ["/items/42", "/items/7", "/health"] {
        send(&tap.address, "GET", path, &[]).await;
    }
    let mut summary = Summary::default();
    for _ in 0..200 {
        let (document, found) = describe(Some(&inspector), None, &Options::default())
            .await
            .unwrap();
        summary = found;
        if summary.requests == 3 {
            let mut paths: Vec<&String> = document["paths"].as_object().unwrap().keys().collect();
            paths.sort();
            assert_eq!(paths, ["/health", "/items/{id}"]);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(summary.requests, 3);
    assert_eq!(summary.hosts, ["demo.example.com"]);
    inspector.shutdown().await;
}

proptest! {
    /// Every observed body conforms to the schema inferred for its operation.
    #[test]
    fn observed_bodies_conform(ids in proptest::collection::vec(1u32..100_000, 1..10), flags in proptest::collection::vec(any::<bool>(), 1..10)) {
        let bodies: Vec<String> = ids
            .iter()
            .zip(flags.iter().cycle())
            .map(|(id, flag)| {
                if *flag {
                    format!(r#"{{"id":{id},"name":"n{id}","active":{flag}}}"#)
                } else {
                    format!(r#"{{"id":{id},"score":{}.5}}"#, id % 7)
                }
            })
            .collect();
        let exchanges: Vec<Exchange> = ids
            .iter()
            .zip(&bodies)
            .map(|(id, body)| {
                exchange("GET", &format!("/items/{id}"), None, 200, Some((JSON, body)), &[])
            })
            .collect();
        let (document, summary) = infer(&exchanges, &Options::default());
        prop_assert_eq!(summary.paths, 1);
        let schema = &document["paths"]["/items/{id}"]["get"]["responses"]["200"]["content"][JSON]["schema"];
        for body in &bodies {
            let value: Value = serde_json::from_str(body).unwrap();
            prop_assert!(conforms(&value, schema), "{} vs {}", body, schema);
        }
    }
}
