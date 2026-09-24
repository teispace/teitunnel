//! A tiny authoritative DNS responder for `.test` names, on loopback only.
//!
//! Registered names (and one level below names with wildcard on) answer `A 127.0.0.1` and
//! `AAAA ::1`; other `.test` names get NXDOMAIN; anything outside `.test` is REFUSED (it
//! isn't a recursive resolver). UDP and TCP (RFC 7766 framing) on the same port. The OS
//! sends `.test` queries here through a per-TLD resolver entry (see
//! [`crate::resolver_config`]).

use std::{
    collections::BTreeMap,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, RwLock},
    time::Duration,
};

use hickory_proto::{
    op::{Edns, Message, MessageType, OpCode, ResponseCode},
    rr::{
        DNSClass, RData, Record, RecordType,
        rdata::{A, AAAA},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::name::{LocalName, Suffix};

/// Default port: unprivileged, and unlikely to clash with mDNS (5353) or local resolvers.
pub const DEFAULT_PORT: u16 = 53535;
/// TTL of answers, short so changes show up quickly.
const TTL: u32 = 5;
/// Largest UDP payload we advertise with EDNS (RFC 9715-style safe size).
const EDNS_PAYLOAD: u16 = 1232;
/// Idle TCP connections are closed after this.
const TCP_IDLE: Duration = Duration::from_secs(10);
/// At most this many TCP connections are served at once.
const MAX_TCP_CONNECTIONS: usize = 64;

/// The names the responder answers for. Share it with whatever updates the registry.
#[derive(Debug, Default)]
pub struct DnsZone {
    names: RwLock<BTreeMap<LocalName, bool>>,
}

impl DnsZone {
    /// An empty zone.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the zone's names: `(name, wildcard)` pairs; non-`.test` names are ignored.
    pub fn set(&self, names: impl IntoIterator<Item = (LocalName, bool)>) {
        let names = names
            .into_iter()
            .filter(|(n, _)| n.suffix() == Suffix::Test)
            .collect();
        *self
            .names
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = names;
    }

    /// Whether `name` (lowercase, no trailing dot) is served.
    #[must_use]
    pub fn contains(&self, name: &LocalName) -> bool {
        let names = self
            .names
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        names.contains_key(name)
            || name
                .parent()
                .is_some_and(|parent| names.get(&parent).copied().unwrap_or(false))
    }
}

/// Answers one DNS message. `None` means "don't reply" (responses, or too short to have an
/// id). Never panics, whatever the input.
#[must_use]
pub fn answer(zone: &DnsZone, packet: &[u8]) -> Option<Vec<u8>> {
    if packet.len() < 12 || packet[2] & 0x80 != 0 {
        // Shorter than a header, or a response (QR set): ignore.
        return None;
    }
    let Ok(request) = Message::from_vec(packet) else {
        let id = u16::from_be_bytes([packet[0], packet[1]]);
        return Message::error_msg(id, OpCode::Query, ResponseCode::FormErr)
            .to_vec()
            .ok();
    };
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.recursion_desired = request.metadata.recursion_desired;
    response.metadata.recursion_available = false;
    if request.edns.is_some() {
        let mut edns = Edns::new();
        edns.set_max_payload(EDNS_PAYLOAD);
        response.edns = Some(edns);
    }
    if request.metadata.message_type != MessageType::Query
        || request.metadata.op_code != OpCode::Query
    {
        response.metadata.response_code = ResponseCode::NotImp;
        return response.to_vec().ok();
    }
    let [query] = request.queries.as_slice() else {
        response.metadata.response_code = ResponseCode::FormErr;
        return response.to_vec().ok();
    };
    response.add_query(query.clone());
    let qname = query.name().to_ascii();
    let in_zone = qname
        .trim_end_matches('.')
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.eq_ignore_ascii_case(Suffix::Test.as_str()));
    if !in_zone || query.query_class() != DNSClass::IN {
        response.metadata.response_code = ResponseCode::Refused;
        return response.to_vec().ok();
    }
    response.metadata.authoritative = true;
    let known = LocalName::parse(&qname, &[Suffix::Test]).is_ok_and(|name| zone.contains(&name));
    if !known {
        response.metadata.response_code = ResponseCode::NXDomain;
        return response.to_vec().ok();
    }
    let owner = query.name().clone();
    match query.query_type() {
        RecordType::A => {
            response.add_answer(Record::from_rdata(
                owner,
                TTL,
                RData::A(A(Ipv4Addr::LOCALHOST)),
            ));
        }
        RecordType::AAAA => {
            response.add_answer(Record::from_rdata(
                owner,
                TTL,
                RData::AAAA(AAAA(Ipv6Addr::LOCALHOST)),
            ));
        }
        RecordType::ANY => {
            response.add_answer(Record::from_rdata(
                owner.clone(),
                TTL,
                RData::A(A(Ipv4Addr::LOCALHOST)),
            ));
            response.add_answer(Record::from_rdata(
                owner,
                TTL,
                RData::AAAA(AAAA(Ipv6Addr::LOCALHOST)),
            ));
        }
        // Known name, other type: NOERROR with no answers (NODATA).
        _ => {}
    }
    response.to_vec().ok()
}

/// A DNS responder failure.
#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    /// The address isn't loopback; the responder never listens on other interfaces.
    #[error("the DNS responder only listens on loopback, not {0}")]
    NotLoopback(SocketAddr),
    /// Binding failed (e.g. the port is in use).
    #[error("binding {addr}: {source}")]
    Bind {
        /// The address.
        addr: SocketAddr,
        /// The error.
        source: io::Error,
    },
}

/// A running responder. Dropping it stops it.
#[derive(Debug)]
pub struct DnsResponder {
    local_addr: SocketAddr,
    cancel: CancellationToken,
    tasks: JoinSet<()>,
}

impl DnsResponder {
    /// Starts answering on `addr` (loopback only; port 0 picks a free port for both UDP and
    /// TCP).
    ///
    /// # Errors
    /// The address isn't loopback, or binding failed.
    pub async fn start(addr: SocketAddr, zone: Arc<DnsZone>) -> Result<Self, DnsError> {
        if !addr.ip().is_loopback() {
            return Err(DnsError::NotLoopback(addr));
        }
        let bind_err = |addr| move |source| DnsError::Bind { addr, source };
        let (udp, tcp) = if addr.port() == 0 {
            bind_same_free_port(addr).await.map_err(bind_err(addr))?
        } else {
            let udp = UdpSocket::bind(addr).await.map_err(bind_err(addr))?;
            let tcp = TcpListener::bind(addr).await.map_err(bind_err(addr))?;
            (udp, tcp)
        };
        let local_addr = udp.local_addr().map_err(bind_err(addr))?;
        let cancel = CancellationToken::new();
        let mut tasks = JoinSet::new();
        tasks.spawn(serve_udp(udp, Arc::clone(&zone), cancel.clone()));
        tasks.spawn(serve_tcp(tcp, zone, cancel.clone()));
        tracing::info!(%local_addr, "local DNS responder started");
        Ok(Self {
            local_addr,
            cancel,
            tasks,
        })
    }

    /// The bound address.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stops and waits for the listeners to finish.
    pub async fn shutdown(mut self) {
        self.cancel.cancel();
        while self.tasks.join_next().await.is_some() {}
    }
}

impl Drop for DnsResponder {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn bind_same_free_port(addr: SocketAddr) -> io::Result<(UdpSocket, TcpListener)> {
    let mut last_err = None;
    for _ in 0..16 {
        let udp = UdpSocket::bind(addr).await?;
        let port_addr = udp.local_addr()?;
        match TcpListener::bind(port_addr).await {
            Ok(tcp) => return Ok((udp, tcp)),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("no free port")))
}

async fn serve_udp(socket: UdpSocket, zone: Arc<DnsZone>, cancel: CancellationToken) {
    let mut buf = vec![0_u8; 4096];
    loop {
        let (len, peer) = tokio::select! {
            () = cancel.cancelled() => return,
            received = socket.recv_from(&mut buf) => match received {
                Ok(r) => r,
                Err(err) => {
                    tracing::debug!(error = %err, "dns udp receive failed");
                    continue;
                }
            },
        };
        let Some(reply) = buf.get(..len).and_then(|packet| answer(&zone, packet)) else {
            continue;
        };
        let reply = if reply.len() > usize::from(EDNS_PAYLOAD) {
            truncated(&reply)
        } else {
            reply
        };
        if let Err(err) = socket.send_to(&reply, peer).await {
            tracing::debug!(error = %err, "dns udp send failed");
        }
    }
}

/// A header-only copy of `reply` with TC set, so the client retries over TCP.
fn truncated(reply: &[u8]) -> Vec<u8> {
    Message::from_vec(reply)
        .ok()
        .and_then(|m| m.truncate().to_vec().ok())
        .unwrap_or_default()
}

async fn serve_tcp(listener: TcpListener, zone: Arc<DnsZone>, cancel: CancellationToken) {
    let mut connections = JoinSet::new();
    loop {
        let stream = tokio::select! {
            () = cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(err) => {
                    tracing::debug!(error = %err, "dns tcp accept failed");
                    continue;
                }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => continue,
        };
        if connections.len() >= MAX_TCP_CONNECTIONS {
            continue;
        }
        connections.spawn(serve_tcp_connection(
            stream,
            Arc::clone(&zone),
            cancel.clone(),
        ));
    }
    connections.shutdown().await;
}

async fn serve_tcp_connection(
    mut stream: TcpStream,
    zone: Arc<DnsZone>,
    cancel: CancellationToken,
) {
    loop {
        let read = async {
            let len = stream.read_u16().await?;
            let mut packet = vec![0_u8; usize::from(len)];
            stream.read_exact(&mut packet).await?;
            Ok::<_, io::Error>(packet)
        };
        let packet = tokio::select! {
            () = cancel.cancelled() => return,
            result = tokio::time::timeout(TCP_IDLE, read) => match result {
                Ok(Ok(packet)) => packet,
                _ => return,
            },
        };
        let Some(reply) = answer(&zone, &packet) else {
            return;
        };
        let Ok(len) = u16::try_from(reply.len()) else {
            return;
        };
        let mut framed = Vec::with_capacity(reply.len() + 2);
        framed.extend_from_slice(&len.to_be_bytes());
        framed.extend_from_slice(&reply);
        if stream.write_all(&framed).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hickory_proto::{op::Query, rr::Name};
    use proptest::prelude::*;

    use super::*;

    fn zone() -> Arc<DnsZone> {
        let zone = Arc::new(DnsZone::new());
        zone.set([
            (LocalName::parse_any("app.test").unwrap(), true),
            (LocalName::parse_any("api.test").unwrap(), false),
            (LocalName::parse_any("ignored.localhost").unwrap(), true),
        ]);
        zone
    }

    fn query(name: &str, ty: RecordType) -> Vec<u8> {
        let mut msg = Message::query();
        msg.metadata.id = 0x1234;
        msg.metadata.recursion_desired = true;
        msg.add_query(Query::query(Name::from_str(name).unwrap(), ty));
        msg.to_vec().unwrap()
    }

    fn ask(name: &str, ty: RecordType) -> Message {
        Message::from_vec(&answer(&zone(), &query(name, ty)).unwrap()).unwrap()
    }

    fn ips(msg: &Message) -> Vec<String> {
        msg.answers.iter().map(|r| r.data.to_string()).collect()
    }

    #[test]
    fn answers_registered_names_and_wildcards() {
        let a = ask("app.test.", RecordType::A);
        assert_eq!(a.metadata.id, 0x1234);
        assert_eq!(a.metadata.message_type, MessageType::Response);
        assert!(a.metadata.authoritative);
        assert!(a.metadata.recursion_desired);
        assert_eq!(a.metadata.response_code, ResponseCode::NoError);
        assert_eq!(ips(&a), ["127.0.0.1"]);
        assert_eq!(ips(&ask("APP.test.", RecordType::AAAA)), ["::1"]);
        assert_eq!(ips(&ask("x.app.test.", RecordType::A)), ["127.0.0.1"]);
        let nodata = ask("app.test.", RecordType::MX);
        assert_eq!(nodata.metadata.response_code, ResponseCode::NoError);
        assert!(nodata.answers.is_empty());
    }

    #[test]
    fn nxdomain_and_refused() {
        assert_eq!(
            ask("x.api.test.", RecordType::A).metadata.response_code,
            ResponseCode::NXDomain
        );
        assert_eq!(
            ask("other.test.", RecordType::A).metadata.response_code,
            ResponseCode::NXDomain
        );
        assert_eq!(
            ask("test.", RecordType::A).metadata.response_code,
            ResponseCode::NXDomain
        );
        assert_eq!(
            ask("example.com.", RecordType::A).metadata.response_code,
            ResponseCode::Refused
        );
        assert_eq!(
            ask("ignored.localhost.", RecordType::A)
                .metadata
                .response_code,
            ResponseCode::Refused
        );
    }

    #[test]
    fn malformed_and_responses() {
        let z = zone();
        assert!(answer(&z, &[]).is_none());
        assert!(answer(&z, &[0; 11]).is_none());
        let mut garbage = vec![0xab, 0xcd, 0x01, 0x00, 0x00, 0x05];
        garbage.extend_from_slice(&[0xff; 20]);
        let reply = Message::from_vec(&answer(&z, &garbage).unwrap()).unwrap();
        assert_eq!(reply.metadata.id, 0xabcd);
        assert_eq!(reply.metadata.response_code, ResponseCode::FormErr);
        let mut response = query("app.test.", RecordType::A);
        response[2] |= 0x80;
        assert!(
            answer(&z, &response).is_none(),
            "never answer responses (no loops)"
        );
    }

    #[tokio::test]
    async fn udp_and_tcp_round_trip_on_loopback() {
        let responder = DnsResponder::start("127.0.0.1:0".parse().unwrap(), zone())
            .await
            .unwrap();
        let addr = responder.local_addr();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(&query("app.test.", RecordType::A), addr)
            .await
            .unwrap();
        let mut buf = [0_u8; 512];
        let (len, _) = client.recv_from(&mut buf).await.unwrap();
        assert_eq!(ips(&Message::from_vec(&buf[..len]).unwrap()), ["127.0.0.1"]);

        let mut tcp = TcpStream::connect(addr).await.unwrap();
        for (name, code) in [
            ("api.test.", ResponseCode::NoError),
            ("nope.test.", ResponseCode::NXDomain),
        ] {
            let q = query(name, RecordType::AAAA);
            tcp.write_u16(u16::try_from(q.len()).unwrap())
                .await
                .unwrap();
            tcp.write_all(&q).await.unwrap();
            let len = tcp.read_u16().await.unwrap();
            let mut reply = vec![0_u8; usize::from(len)];
            tcp.read_exact(&mut reply).await.unwrap();
            assert_eq!(
                Message::from_vec(&reply).unwrap().metadata.response_code,
                code
            );
        }
        responder.shutdown().await;
    }

    #[tokio::test]
    async fn refuses_non_loopback() {
        let err = DnsResponder::start("0.0.0.0:0".parse().unwrap(), zone())
            .await
            .unwrap_err();
        assert!(matches!(err, DnsError::NotLoopback(_)));
    }

    proptest! {
        #[test]
        fn arbitrary_packets_never_panic(packet in proptest::collection::vec(any::<u8>(), 0..600)) {
            let _ = answer(&zone(), &packet);
        }

        #[test]
        fn mutated_queries_never_panic(index in 0_usize..40, byte in any::<u8>()) {
            let mut packet = query("x.app.test.", RecordType::A);
            if let Some(b) = packet.get_mut(index) {
                *b = byte;
            }
            let _ = answer(&zone(), &packet);
        }
    }
}
