use std::net::Ipv4Addr;

use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::catalog::{DHCP_PROCESS, SERVICES, Service};
use crate::{
    features::{devices::describe, paths::device},
    packet_tracer::{PacketTracer, PtError, expect_bool, expect_integer, expect_text},
};

const DEFAULT_PORT: &str = "FastEthernet0";
const NO_ADDRESS: &str = "0.0.0.0";
const DEFAULT_MAX_USERS: i32 = 256;
const FTP_ALL_PERMISSIONS: &str = "RWDNL";
const DNS_TYPES: &[(i64, &str)] = &[(1, "A"), (2, "NS"), (5, "CNAME"), (6, "SOA")];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ServicesRequest {
    /// Server name.
    pub device: String,
    /// Port whose DHCP service to report. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ServiceState {
    pub service: Service,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ServicesList {
    pub device: String,
    pub services: Vec<ServiceState>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetServiceRequest {
    pub device: String,
    pub service: Service,
    pub enabled: bool,
    /// For DHCP: the port the service runs on. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PoolRequest {
    /// Pool name, for example `VLAN10`. The server's default pool is `serverPool`.
    pub name: String,
    /// Default gateway handed to clients.
    pub gateway: String,
    /// First address to lease.
    pub start_ip: String,
    pub mask: String,
    #[serde(default)]
    pub dns: Option<String>,
    /// How many addresses to lease. Defaults to 256.
    #[serde(default)]
    pub max_users: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DhcpRequest {
    pub device: String,
    /// Port the DHCP service runs on. Defaults to `FastEthernet0`.
    #[serde(default)]
    pub port: Option<String>,
    /// Switch the service on or off. Defaults to on.
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub pools: Vec<PoolRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DhcpPool {
    pub name: String,
    pub network: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
    pub start_ip: String,
    pub end_ip: String,
    pub max_users: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DhcpServer {
    pub device: String,
    pub port: String,
    pub enabled: bool,
    pub pools: Vec<DhcpPool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum RecordType {
    A,
    #[serde(rename = "CNAME")]
    Cname,
    #[serde(rename = "NS")]
    Ns,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct DnsRecord {
    /// Domain name, for example `www.gamc.bo`.
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: RecordType,
    /// IPv4 address for A, host name for CNAME, server name for NS.
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DnsRequest {
    pub device: String,
    /// Switch the service on or off. Defaults to on.
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub records: Vec<DnsRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DnsEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct DnsServer {
    pub device: String,
    pub enabled: bool,
    pub records: Vec<DnsEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WebPageRequest {
    pub device: String,
    /// Page name, for example `index.html`.
    pub url: String,
    /// HTML contents.
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WebPage {
    pub device: String,
    pub url: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserService {
    Ftp,
    Email,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UserRequest {
    pub device: String,
    pub service: UserService,
    pub username: String,
    pub password: String,
    /// FTP permissions: any of R(ead) W(rite) D(elete) N (rename) L(ist). Defaults to `RWDNL`.
    #[serde(default)]
    pub permissions: Option<String>,
    /// Email: the server's mail domain, for example `gamc.bo`.
    #[serde(default)]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct UserAdded {
    pub device: String,
    pub service: UserService,
    pub username: String,
}

fn process(name: &str, kind: &str) -> Call {
    device(name).method("getProcess", [Value::string(kind)])
}

fn dhcp(name: &str, port: &str) -> Call {
    process(name, DHCP_PROCESS).method("getDhcpServerProcessByPortName", [Value::string(port)])
}

fn port_of(port: Option<&str>) -> String {
    port.map(str::trim)
        .filter(|port| !port.is_empty())
        .unwrap_or(DEFAULT_PORT)
        .to_owned()
}

async fn server<P: PacketTracer>(packet_tracer: &P, name: &str) -> Result<String, PtError> {
    let name = name.trim().to_owned();
    describe(packet_tracer, &name).await?;
    if packet_tracer
        .call(process(&name, DHCP_PROCESS).method("getClassName", []))
        .await
        .is_err()
    {
        return Err(PtError::InvalidInput(format!(
            "`{name}` has no server services; use a Server-PT"
        )));
    }
    Ok(name)
}

async fn enabled<P: PacketTracer>(packet_tracer: &P, call: Call) -> Result<bool, PtError> {
    let reply = packet_tracer.call(call).await?;
    expect_bool(&reply, "service state")
}

pub async fn list_services<P: PacketTracer>(
    packet_tracer: &P,
    request: &ServicesRequest,
) -> Result<ServicesList, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let port = port_of(request.port.as_deref());
    let mut services = vec![ServiceState {
        service: Service::Dhcp,
        enabled: enabled(packet_tracer, dhcp(&name, &port).method("isEnable", [])).await?,
    }];
    for spec in SERVICES {
        match enabled(
            packet_tracer,
            process(&name, spec.process).method(spec.getter, []),
        )
        .await
        {
            Ok(on) => services.push(ServiceState {
                service: spec.service,
                enabled: on,
            }),
            Err(PtError::NotFound(_) | PtError::Rejected(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(ServicesList {
        device: name,
        services,
    })
}

pub async fn set_service<P: PacketTracer>(
    packet_tracer: &P,
    request: &SetServiceRequest,
) -> Result<ServiceState, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let (setter, getter) = if request.service == Service::Dhcp {
        let call = dhcp(&name, &port_of(request.port.as_deref()));
        (
            call.clone()
                .method("setEnable", [Value::Bool(request.enabled)]),
            call.method("isEnable", []),
        )
    } else {
        let spec = SERVICES
            .iter()
            .find(|spec| spec.service == request.service)
            .ok_or_else(|| PtError::InvalidInput("unknown service".into()))?;
        let call = process(&name, spec.process);
        (
            call.clone()
                .method(spec.setter, [Value::Bool(request.enabled)]),
            call.method(spec.getter, []),
        )
    };
    packet_tracer.call(setter).await?;
    Ok(ServiceState {
        service: request.service,
        enabled: enabled(packet_tracer, getter).await?,
    })
}

fn address(text: &str, what: &str) -> Result<Ipv4Addr, PtError> {
    text.trim()
        .parse()
        .map_err(|_| PtError::InvalidInput(format!("{what} `{text}` is not an IPv4 address")))
}

pub async fn configure_dhcp<P: PacketTracer>(
    packet_tracer: &P,
    request: &DhcpRequest,
) -> Result<DhcpServer, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let port = port_of(request.port.as_deref());
    let service = dhcp(&name, &port);
    for pool in &request.pools {
        let gateway = address(&pool.gateway, "gateway")?;
        let start = address(&pool.start_ip, "start_ip")?;
        let mask = address(&pool.mask, "mask")?;
        let dns = match pool
            .dns
            .as_deref()
            .map(str::trim)
            .filter(|dns| !dns.is_empty())
        {
            Some(dns) => address(dns, "dns")?,
            None => Ipv4Addr::UNSPECIFIED,
        };
        let max_users = pool.max_users.unwrap_or(DEFAULT_MAX_USERS);
        let pool_name = pool.name.trim();
        if pool_name.is_empty() {
            return Err(PtError::InvalidInput("a pool needs a name".into()));
        }
        let existing = service
            .clone()
            .method("getPool", [Value::string(pool_name)]);
        if packet_tracer
            .call(existing.clone().method("getDhcpPoolName", []))
            .await
            .is_ok()
        {
            let network = Ipv4Addr::from(u32::from(start) & u32::from(mask));
            for call in [
                existing
                    .clone()
                    .method("setNetworkMask", [Value::Ip(network), Value::Ip(mask)]),
                existing
                    .clone()
                    .method("setDefaultRouter", [Value::Ip(gateway)]),
                existing.clone().method("setDnsServerIp", [Value::Ip(dns)]),
                existing.clone().method("setStartIp", [Value::Ip(start)]),
                existing
                    .clone()
                    .method("setMaxUsers", [Value::Int(max_users)]),
            ] {
                packet_tracer.call(call).await?;
            }
        } else {
            packet_tracer
                .call(service.clone().method(
                    "addNewPool",
                    [
                        Value::string(pool_name),
                        Value::string(gateway.to_string()),
                        Value::string(dns.to_string()),
                        Value::string(start.to_string()),
                        Value::string(mask.to_string()),
                        Value::Int(max_users),
                        Value::string(NO_ADDRESS),
                        Value::string(NO_ADDRESS),
                    ],
                ))
                .await?;
        }
    }
    packet_tracer
        .call(
            service
                .clone()
                .method("setEnable", [Value::Bool(request.enabled.unwrap_or(true))]),
        )
        .await?;
    Ok(DhcpServer {
        enabled: enabled(packet_tracer, service.clone().method("isEnable", [])).await?,
        pools: read_pools(packet_tracer, &service).await?,
        device: name,
        port,
    })
}

async fn read_pools<P: PacketTracer>(
    packet_tracer: &P,
    service: &Call,
) -> Result<Vec<DhcpPool>, PtError> {
    let count = packet_tracer
        .call(service.clone().method("getPoolCount", []))
        .await?;
    let mut pools = Vec::new();
    for index in 0..expect_integer(&count, "pool count")? {
        let pool = service.clone().method(
            "getPoolAt",
            [Value::Int(i32::try_from(index).unwrap_or_default())],
        );
        let get = |method: &str| packet_tracer.call(pool.clone().method(method, []));
        let (name, network, mask, gateway, dns, start, end, max) = tokio::try_join!(
            get("getDhcpPoolName"),
            get("getNetworkAddress"),
            get("getSubnetMask"),
            get("getDefaultRouter"),
            get("getDnsServerIp"),
            get("getStartIp"),
            get("getEndIp"),
            get("getMaxUsers"),
        )?;
        let ip = |value: &Value| value.as_ip().map(|ip| ip.to_string()).unwrap_or_default();
        pools.push(DhcpPool {
            name: expect_text(&name, "pool name")?,
            network: ip(&network),
            mask: ip(&mask),
            gateway: ip(&gateway),
            dns: ip(&dns),
            start_ip: ip(&start),
            end_ip: ip(&end),
            max_users: expect_integer(&max, "max users")?,
        });
    }
    Ok(pools)
}

pub async fn configure_dns<P: PacketTracer>(
    packet_tracer: &P,
    request: &DnsRequest,
) -> Result<DnsServer, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let dns = process(&name, "DnsServerProcess");
    for record in &request.records {
        let (method, value) = match record.record_type {
            RecordType::A => (
                "addARecordToNameServerDb",
                address(&record.value, "A record")?.to_string(),
            ),
            RecordType::Cname => ("addCNAMEToNameServerDb", record.value.trim().to_owned()),
            RecordType::Ns => ("addNSRecordToNameServerDb", record.value.trim().to_owned()),
        };
        let added = packet_tracer
            .call(dns.clone().method(
                method,
                [Value::string(record.name.trim()), Value::string(value)],
            ))
            .await?;
        if !expect_bool(&added, "record added")? {
            return Err(PtError::Rejected(format!(
                "Packet Tracer refused the {:?} record for `{}`; it may already exist",
                record.record_type, record.name
            )));
        }
    }
    packet_tracer
        .call(
            dns.clone()
                .method("setEnable", [Value::Bool(request.enabled.unwrap_or(true))]),
        )
        .await?;
    Ok(DnsServer {
        enabled: enabled(packet_tracer, dns.clone().method("isEnabled", [])).await?,
        records: read_records(packet_tracer, &dns).await?,
        device: name,
    })
}

async fn read_records<P: PacketTracer>(
    packet_tracer: &P,
    dns: &Call,
) -> Result<Vec<DnsEntry>, PtError> {
    let count = packet_tracer
        .call(dns.clone().method("getSizeOfNameServerDb", []))
        .await?;
    let mut records = Vec::new();
    for index in 0..expect_integer(&count, "record count")? {
        let record = packet_tracer
            .call(dns.clone().method(
                "getRrFromNameServerDbAt",
                [Value::Int(i32::try_from(index).unwrap_or_default())],
            ))
            .await?;
        let Value::Data { fields, .. } = record else {
            return Err(PtError::UnexpectedReply(format!(
                "a DNS record should be a value object, got {record:?}"
            )));
        };
        let name = fields
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let code = fields.get(1).and_then(Value::as_i64).unwrap_or_default();
        let value = match fields.last() {
            Some(Value::Ip(ip)) => ip.to_string(),
            Some(other) => other.as_str().unwrap_or_default().to_owned(),
            None => String::new(),
        };
        records.push(DnsEntry {
            name,
            record_type: DNS_TYPES
                .iter()
                .find(|(value, _)| *value == code)
                .map_or_else(|| format!("TYPE{code}"), |(_, label)| (*label).to_owned()),
            value,
        });
    }
    Ok(records)
}

pub async fn set_web_page<P: PacketTracer>(
    packet_tracer: &P,
    request: &WebPageRequest,
) -> Result<WebPage, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let url = request.url.trim();
    if url.is_empty() {
        return Err(PtError::InvalidInput(
            "a page needs a url such as index.html".into(),
        ));
    }
    let http = process(&name, "HttpServer");
    packet_tracer
        .call(http.clone().method(
            "setPageContents",
            [Value::string(url), Value::string(&request.contents)],
        ))
        .await?;
    let stored = packet_tracer
        .call(http.method("getPage", [Value::string(url)]))
        .await?;
    let stored = expect_text(&stored, "page")?;
    if stored != request.contents {
        return Err(PtError::Rejected(format!(
            "`{url}` was not stored as given"
        )));
    }
    Ok(WebPage {
        device: name,
        url: url.to_owned(),
        bytes: stored.len(),
    })
}

pub async fn add_user<P: PacketTracer>(
    packet_tracer: &P,
    request: &UserRequest,
) -> Result<UserAdded, PtError> {
    let name = server(packet_tracer, &request.device).await?;
    let username = request.username.trim();
    if username.is_empty() || request.password.is_empty() {
        return Err(PtError::InvalidInput(
            "username and password are required".into(),
        ));
    }
    match request.service {
        UserService::Ftp => {
            let permissions = request
                .permissions
                .as_deref()
                .unwrap_or(FTP_ALL_PERMISSIONS)
                .to_uppercase();
            if permissions.is_empty() || !permissions.chars().all(|flag| "RWDNL".contains(flag)) {
                return Err(PtError::InvalidInput(
                    "FTP permissions are letters from RWDNL".into(),
                ));
            }
            let accounts = process(&name, "FtpServer").method("getFtpUserAccountManager", []);
            packet_tracer
                .call(accounts.clone().method(
                    "addFtpUser",
                    [
                        Value::string(username),
                        Value::string(&request.password),
                        Value::string(permissions),
                    ],
                ))
                .await?;
            let exists = packet_tracer
                .call(accounts.method("isExistingUser", [Value::string(username)]))
                .await?;
            if !expect_bool(&exists, "user exists")? {
                return Err(PtError::Rejected(format!(
                    "the FTP account `{username}` was not created"
                )));
            }
        }
        UserService::Email => {
            if let Some(domain) = request
                .domain
                .as_deref()
                .map(str::trim)
                .filter(|domain| !domain.is_empty())
            {
                packet_tracer
                    .call(
                        process(&name, "SmtpServer")
                            .method("setServerDomainName", [Value::string(domain)]),
                    )
                    .await?;
            }
            let added = packet_tracer
                .call(process(&name, "EmailServer").method(
                    "addUser",
                    [Value::string(username), Value::string(&request.password)],
                ))
                .await?;
            if !expect_bool(&added, "user added")? {
                return Err(PtError::Rejected(format!(
                    "the email account `{username}` was not created; it may already exist"
                )));
            }
        }
    }
    Ok(UserAdded {
        device: name,
        service: request.service,
        username: username.to_owned(),
    })
}
