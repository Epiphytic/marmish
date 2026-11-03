//! Explicit setup-only SRV discovery. No account or transport is created here.
use crate::{
    Error, Result,
    command_target::validate_srv_name,
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use futures_util::StreamExt;
use hickory_resolver::{
    NameServerPool, PoolContext, TlsConfig,
    config::{NameServerConfig, ResolverConfig, ResolverOpts, ServerOrderingStrategy},
    net::{runtime::TokioRuntimeProvider, xfer::DnsHandle},
    proto::{
        op::{DnsRequestOptions, Message, Query, ResponseCode},
        rr::{DNSClass, Name, RData, Record, RecordType},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(5);
const RECORD_LIMIT: usize = 16;
fn invalid(category: &'static str) -> Error {
    Error::Invalid(category)
}

/// Called only after target selection rejects always/onion routes. The dedicated
/// runtime is destroyed before this function returns any resolver result.
pub(crate) async fn resolve(names: &[String]) -> Result<Vec<String>> {
    let names = owners(names)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("marmish-srv-setup".into())
        .spawn(move || {
            let began = Instant::now();
            let result = hickory_resolver::system_conf::read_system_conf()
                .map_err(|_| invalid("srv_backend_unavailable"))
                .and_then(|(config, _)| rebuild(config))
                .and_then(|config| run_setup(names, config, began + BUDGET));
            let _ = send.send(result);
        })
        .map_err(|_| invalid("srv_backend_unavailable"))?;
    let result = receive
        .await
        .map_err(|_| invalid("srv_backend_unavailable"));
    thread
        .join()
        .map_err(|_| invalid("srv_backend_unavailable"))?;
    result?
}
fn owners(names: &[String]) -> Result<Vec<String>> {
    if names.is_empty() || names.len() > 16 || names.iter().map(String::len).sum::<usize>() > 4096 {
        return Err(invalid("invalid_srv_name"));
    }
    let mut unique = BTreeSet::new();
    let mut result = Vec::new();
    for name in names {
        let name = validate_srv_name(name)?;
        if !unique.insert(name.clone()) {
            return Err(invalid("invalid_srv_name"));
        }
        result.push(name);
    }
    Ok(result)
}
fn usable(ip: IpAddr) -> bool {
    !ip.is_unspecified()
        && !ip.is_multicast()
        && !matches!(ip,IpAddr::V6(v) if v.is_unicast_link_local())
}
fn rebuild(config: ResolverConfig) -> Result<ResolverConfig> {
    let (_, _, servers) = config.into_parts();
    let mut ips = Vec::new();
    for server in servers {
        if usable(server.ip) && !ips.contains(&server.ip) {
            ips.push(server.ip);
        }
        if ips.len() == 3 {
            break;
        }
    }
    if ips.is_empty() {
        return Err(invalid("srv_backend_unavailable"));
    }
    Ok(ResolverConfig::from_name_servers(
        ips.into_iter().map(NameServerConfig::udp_and_tcp).collect(),
    ))
}
fn options() -> ResolverOpts {
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(1);
    opts.num_concurrent_reqs = 1;
    opts.max_active_requests = 1;
    opts.case_randomization = false;
    opts.server_ordering_strategy = ServerOrderingStrategy::UserProvidedOrder;
    opts
}
fn run_setup(names: Vec<String>, config: ResolverConfig, deadline: Instant) -> Result<Vec<String>> {
    if Instant::now() >= deadline {
        return Err(invalid("srv_timeout"));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| invalid("srv_backend_unavailable"))?;
    let result = runtime.block_on(async {
        let opts = options();
        let mut request_options = DnsRequestOptions::default();
        request_options.recursion_desired = opts.recursion_desired;
        request_options.use_edns = opts.edns0;
        request_options.edns_payload_len = opts.edns_payload_len;
        let pool = NameServerPool::from_config(
            config.into_parts().2,
            Arc::new(PoolContext::new(
                opts,
                TlsConfig::new().map_err(|_| invalid("srv_backend_unavailable"))?,
            )),
            TokioRuntimeProvider::default(),
        );
        let mut count = 0;
        let mut targets = Vec::new();
        for owner in names {
            if Instant::now() >= deadline {
                return Err(invalid("srv_timeout"));
            }
            let name = Name::from_ascii(&owner).map_err(|_| invalid("invalid_srv_name"))?;
            let query = Query::query(name, RecordType::SRV);
            let mut response = pool.lookup(query.clone(), request_options);
            let response =
                tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), response.next())
                    .await
                    .map_err(|_| invalid("srv_timeout"))?
                    .ok_or(invalid("srv_resolution_failed"))?
                    .map_err(|_| invalid("srv_resolution_failed"))?;
            validate_response(&response, &query)?;
            targets.extend(answers(&owner, &response.answers, &mut count)?);
        }
        canonical(targets)
    });
    // Hickory's provider owns spawned tasks; dropping only a lookup is not enough.
    // Runtime destruction cancels tasks and closes resolver sockets before return.
    drop(runtime);
    result
}
fn validate_response(response: &Message, query: &Query) -> Result<()> {
    if response.response_code != ResponseCode::NoError
        || response.truncation
        || response.queries.as_slice() != std::slice::from_ref(query)
    {
        return Err(invalid("srv_invalid_response"));
    }
    Ok(())
}
fn dns_name(name: &Name) -> Result<String> {
    let value = name.to_ascii().to_ascii_lowercase();
    if !name.is_fqdn() || !value.is_ascii() || value.contains('\\') || value.len() > 255 {
        return Err(invalid("srv_invalid_response"));
    }
    Ok(value)
}
fn target(name: &Name, port: u16) -> Result<String> {
    if name.is_root() {
        return Err(invalid("srv_unavailable"));
    }
    let absolute = dns_name(name)?;
    let host = absolute
        .strip_suffix('.')
        .ok_or(invalid("srv_invalid_response"))?;
    if port == 0
        || host.parse::<IpAddr>().is_ok()
        || host == "onion"
        || host.ends_with(".onion")
        || !host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid("srv_invalid_response"));
    }
    Ok(format!("wss://{host}:{port}/"))
}
fn answers(owner: &str, records: &[Record], count: &mut usize) -> Result<Vec<String>> {
    if records.len() > RECORD_LIMIT + 8 {
        return Err(invalid("srv_response_limit"));
    }
    let mut chain = BTreeMap::new();
    let mut services = Vec::new();
    for record in records {
        if record.dns_class != DNSClass::IN {
            return Err(invalid("srv_invalid_response"));
        }
        let owner = dns_name(&record.name)?;
        match &record.data {
            RData::CNAME(cname) => {
                let next = dns_name(&cname.0)?;
                if chain.insert(owner, next).is_some() || chain.len() > 8 {
                    return Err(invalid("srv_invalid_response"));
                }
            }
            RData::SRV(srv) => {
                *count += 1;
                if *count > RECORD_LIMIT {
                    return Err(invalid("srv_response_limit"));
                }
                services.push((owner, target(&srv.target, srv.port)?));
            }
            _ => return Err(invalid("srv_invalid_response")),
        }
    }
    let mut current = owner.to_owned();
    let mut visited = BTreeSet::new();
    while let Some(next) = chain.get(&current) {
        if !visited.insert(current.clone()) {
            return Err(invalid("srv_invalid_response"));
        }
        current = next.clone();
    }
    if visited.len() != chain.len()
        || services.is_empty()
        || services.iter().any(|(name, _)| name != &current)
    {
        return Err(invalid("srv_invalid_response"));
    }
    Ok(services.into_iter().map(|(_, url)| url).collect())
}
fn canonical(targets: Vec<String>) -> Result<Vec<String>> {
    let policy = RelayPolicy::parse(&targets, RouteMode::Off, TransportLimits::default())
        .map_err(|_| invalid("srv_invalid_response"))?;
    if policy
        .urls()
        .iter()
        .any(|url| !matches!(url.host(), Some(url::Host::Domain(_))))
    {
        return Err(invalid("srv_invalid_response"));
    }
    Ok(policy.urls().iter().map(ToString::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_resolver::proto::{
        op::{Message, OpCode},
        rr::rdata::{CNAME, SRV},
    };
    use std::{
        net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
        sync::mpsc,
    };
    fn name(value: &str) -> Name {
        Name::from_ascii(value).unwrap()
    }
    fn srv(owner: &str, host: &str, port: u16) -> Record {
        Record::from_rdata(name(owner), 0, RData::SRV(SRV::new(9, 4, port, name(host))))
    }
    fn cname(owner: &str, target: &str) -> Record {
        Record::from_rdata(name(owner), 0, RData::CNAME(CNAME(name(target))))
    }
    #[test]
    fn explicit_owners_and_system_configuration_have_closed_bounds() {
        assert_eq!(
            owners(&["_Nostr._TCP.Example.COM".into()]).unwrap(),
            vec!["_nostr._tcp.example.com."]
        );
        for names in [
            vec![],
            vec!["_a._tcp.example.com".into(), "_A._TCP.example.com.".into()],
            vec!["_a._tcp.hidden.onion".into()],
            vec!["_a._udp.example.com".into()],
            vec!["_a._tcp.example.com".into(); 17],
        ] {
            assert!(owners(&names).is_err());
        }
        let ips = [
            "0.0.0.0",
            "224.0.0.1",
            "fe80::1",
            "::",
            "ff02::1",
            "127.0.0.1",
            "192.0.2.1",
            "192.0.2.2",
            "192.0.2.3",
        ];
        let config = ResolverConfig::from_name_servers(
            ips.iter()
                .map(|ip| NameServerConfig::udp_and_tcp(ip.parse().unwrap()))
                .collect(),
        );
        let rebuilt = rebuild(config).unwrap();
        let servers = rebuilt.name_servers();
        assert_eq!(
            servers.iter().map(|s| s.ip).collect::<Vec<_>>(),
            vec![
                "127.0.0.1".parse::<IpAddr>().unwrap(),
                "192.0.2.1".parse().unwrap(),
                "192.0.2.2".parse().unwrap()
            ]
        );
        assert!(rebuilt.search().is_empty());
        assert!(
            servers
                .iter()
                .all(|s| s.connections.len() == 2 && s.connections.iter().all(|c| c.port == 53))
        );
        assert!(rebuild(ResolverConfig::from_name_servers(vec![])).is_err());
        assert!(usable(IpAddr::V6(Ipv6Addr::LOCALHOST))); // loopback is permitted system DNS.
        let opts = options();
        assert_eq!(opts.timeout, Duration::from_secs(1));
        assert_eq!((opts.num_concurrent_reqs, opts.max_active_requests), (1, 1));
        assert!(!opts.case_randomization);
    }
    #[test]
    fn srv_answers_validate_complete_cname_chain_and_target_identity() {
        let owner = "_nostr._tcp.example.com.";
        let records = vec![
            cname(owner, "_nostr._tcp.alias.example."),
            srv("_nostr._tcp.alias.example.", "Relay.Example.", 443),
            srv("_nostr._tcp.alias.example.", "second.example.", 8443),
        ];
        let urls = answers(owner, &records, &mut 0).unwrap();
        assert_eq!(
            canonical(urls).unwrap(),
            vec!["wss://relay.example/", "wss://second.example:8443/"]
        );
        let invalids = [
            vec![],
            vec![srv(owner, ".", 443)],
            vec![srv(owner, "relay.example.", 0)],
            vec![srv(owner, "127.0.0.1.", 443)],
            vec![srv(owner, "hidden.onion.", 443)],
            vec![srv(owner, "bad_name.example.", 443)],
            vec![srv("_else._tcp.example.", "relay.example.", 443)],
            vec![
                cname(owner, "alias.example."),
                cname("alias.example.", owner),
                srv(owner, "relay.example.", 443),
            ],
            vec![
                cname("unrelated.example.", "alias.example."),
                srv(owner, "relay.example.", 443),
            ],
            vec![
                cname(owner, "alias.example."),
                srv(owner, "relay.example.", 443),
            ],
            vec![
                cname(owner, "a.example."),
                cname(owner, "b.example."),
                srv("a.example.", "relay.example.", 443),
            ],
        ];
        for records in invalids {
            assert!(answers(owner, &records, &mut 0).is_err());
        }
        for numeric in ["2130706433.", "0x7f.1.", "1.2.3."] {
            assert!(
                canonical(answers(owner, &[srv(owner, numeric, 443)], &mut 0).unwrap()).is_err()
            );
        }
        let mut non_in = srv(owner, "relay.example.", 443);
        non_in.dns_class = DNSClass::CH;
        assert!(answers(owner, &[non_in], &mut 0).is_err());
        let records = vec![srv(owner, "relay.example.", 443); 16];
        let mut count = 0;
        assert_eq!(answers(owner, &records, &mut count).unwrap().len(), 16);
        assert!(matches!(
            answers(owner, &[srv(owner, "relay.example.", 443)], &mut count),
            Err(Error::Invalid("srv_response_limit"))
        ));
        let mut deep = Vec::new();
        let mut previous = owner.to_owned();
        for i in 0..9 {
            let next = format!("hop{i}.example.");
            deep.push(cname(&previous, &next));
            previous = next;
        }
        deep.push(srv(&previous, "relay.example.", 443));
        assert!(answers(owner, &deep, &mut 0).is_err());
    }
    #[test]
    fn raw_response_requires_exact_question_and_success() {
        let query = Query::query(
            Name::from_ascii("_nostr._tcp.example.com.").unwrap(),
            RecordType::SRV,
        );
        let mut valid = Message::response(7, OpCode::Query);
        valid.add_query(query.clone());
        valid.add_answer(srv("_nostr._tcp.example.com.", "relay.example.", 443));
        assert!(validate_response(&valid, &query).is_ok());
        for kind in 0..7 {
            let mut invalid = valid.clone();
            match kind {
                0 => invalid.queries[0].name = Name::from_ascii("other.example.").unwrap(),
                1 => invalid.queries[0].query_type = RecordType::A,
                2 => invalid.queries[0].query_class = DNSClass::CH,
                3 => invalid.queries.clear(),
                4 => invalid.queries.push(query.clone()),
                5 => invalid.metadata.truncation = true,
                _ => invalid.metadata.response_code = ResponseCode::NXDomain,
            }
            assert!(matches!(
                validate_response(&invalid, &query),
                Err(Error::Invalid("srv_invalid_response"))
            ));
        }
    }
    #[test]
    fn raw_pool_never_queries_response_directed_names() {
        let owner = "_nostr._tcp.example.com.";
        for (records, code, expected) in [
            (
                vec![
                    cname(owner, "_nostr._tcp.alias.example."),
                    srv("_nostr._tcp.alias.example.", "relay.example.", 443),
                ],
                ResponseCode::NoError,
                true,
            ),
            (
                vec![cname(owner, "_nostr._tcp.alias.example.")],
                ResponseCode::NoError,
                false,
            ),
            (
                vec![
                    cname(owner, "_nostr._tcp.alias.example."),
                    srv("_nostr._tcp.alias.example.", "private.onion.", 443),
                ],
                ResponseCode::NoError,
                false,
            ),
            (
                vec![srv(owner, "2130706433.", 443)],
                ResponseCode::NoError,
                false,
            ),
            (
                vec![
                    cname(owner, "_nostr._tcp.alias.example."),
                    srv("_nostr._tcp.alias.example.", "relay.example.", 443),
                ],
                ResponseCode::NXDomain,
                false,
            ),
        ] {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let config = local_config(socket.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let mut bytes = [0; 4096];
                let (len, peer) = socket.recv_from(&mut bytes).unwrap();
                let query = Message::from_vec(&bytes[..len]).unwrap();
                assert_eq!(query.queries[0].name().to_ascii(), owner);
                let mut response = Message::response(query.id, OpCode::Query);
                response.metadata.response_code = code;
                response.add_query(query.queries[0].clone());
                for record in records {
                    response.add_answer(record);
                }
                socket.send_to(&response.to_vec().unwrap(), peer).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                let error = socket.recv_from(&mut bytes).unwrap_err();
                assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ));
            });
            let result = run_setup(
                vec![owner.into()],
                config,
                Instant::now() + Duration::from_secs(1),
            );
            assert_eq!(result.is_ok(), expected);
            if expected {
                assert_eq!(result.unwrap(), vec!["wss://relay.example/"]);
            }
            server.join().unwrap();
        }
    }
    fn local_config(address: SocketAddr) -> ResolverConfig {
        let mut server = NameServerConfig::udp(address.ip());
        server.connections[0].port = address.port();
        ResolverConfig::from_name_servers(vec![server])
    }
    fn fixture(
        reply: bool,
    ) -> (
        ResolverConfig,
        std::thread::JoinHandle<SocketAddr>,
        mpsc::Receiver<String>,
    ) {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let config = local_config(socket.local_addr().unwrap());
        let (send, receive) = mpsc::channel();
        let task = std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            let (len, peer) = socket.recv_from(&mut bytes).unwrap();
            let query = Message::from_vec(&bytes[..len]).unwrap();
            let requested = query.queries[0].name().to_ascii();
            send.send(requested.clone()).unwrap();
            if reply {
                let mut response = Message::response(query.id, OpCode::Query);
                response.add_query(query.queries[0].clone());
                let mut answer = srv(&requested, "relay.example.", 8443);
                answer.dns_class = DNSClass::IN;
                response.add_answer(answer);
                socket.send_to(&response.to_vec().unwrap(), peer).unwrap();
            }
            peer
        });
        (config, task, receive)
    }
    #[test]
    fn local_dns_is_absolute_uncached_and_runtime_closes_sockets_on_success() {
        for _ in 0..2 {
            let (config, task, requested) = fixture(true);
            let result = run_setup(
                vec!["_nostr._tcp.example.com.".into()],
                config,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(result, vec!["wss://relay.example:8443/"]);
            assert_eq!(requested.recv().unwrap(), "_nostr._tcp.example.com.");
            let client = task.join().unwrap();
            let closed = UdpSocket::bind(client).unwrap();
            drop(closed);
        }
    }
    #[test]
    fn original_deadline_stops_dns_and_closes_runtime_socket_on_timeout() {
        let (config, task, requested) = fixture(false);
        let result = run_setup(
            vec!["_nostr._tcp.example.com.".into()],
            config,
            Instant::now() + Duration::from_millis(100),
        );
        assert!(matches!(result, Err(Error::Invalid("srv_timeout"))));
        assert_eq!(requested.recv().unwrap(), "_nostr._tcp.example.com.");
        let client = task.join().unwrap();
        UdpSocket::bind(client).unwrap();
        let config = local_config("127.0.0.1:1".parse().unwrap());
        assert!(matches!(
            run_setup(
                vec!["_nostr._tcp.example.com.".into()],
                config,
                Instant::now()
            ),
            Err(Error::Invalid("srv_timeout"))
        ));
    }
}
