use std::{collections::BTreeMap, net::Ipv4Addr};

use ptmp::{Step, TypeCode, Value};

use super::remote::{Remote, check_args, int_arg, no_args};

#[derive(Debug, Clone)]
struct Pool {
    name: String,
    network: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
    start: Ipv4Addr,
    max_users: i32,
}

impl Pool {
    fn end(&self) -> Ipv4Addr {
        let offset = u32::try_from(self.max_users.max(1) - 1).unwrap_or_default();
        Ipv4Addr::from(u32::from(self.start).saturating_add(offset))
    }
}

#[derive(Debug, Clone)]
pub(super) struct Services {
    enabled: BTreeMap<&'static str, bool>,
    dhcp_enabled: bool,
    pools: Vec<Pool>,
    records: Vec<(String, i32, String)>,
    pages: BTreeMap<String, String>,
    ftp_users: BTreeMap<String, (String, String)>,
    email_users: BTreeMap<String, String>,
    mail_domain: String,
}

impl Default for Services {
    fn default() -> Self {
        let on = |name| (name, true);
        Self {
            enabled: BTreeMap::from([
                ("DnsServerProcess", false),
                on("HttpServer"),
                on("HttpsServer"),
                on("FtpServer"),
                on("SmtpServer"),
                on("Pop3Server"),
                on("NtpServer"),
                on("SyslogServer"),
                on("TftpServer"),
            ]),
            dhcp_enabled: false,
            pools: vec![Pool {
                name: "serverPool".into(),
                network: Ipv4Addr::UNSPECIFIED,
                mask: Ipv4Addr::UNSPECIFIED,
                gateway: Ipv4Addr::UNSPECIFIED,
                dns: Ipv4Addr::UNSPECIFIED,
                start: Ipv4Addr::UNSPECIFIED,
                max_users: 0,
            }],
            records: Vec::new(),
            pages: BTreeMap::from([(
                "index.html".to_owned(),
                "<html>Cisco Packet Tracer</html>".to_owned(),
            )]),
            ftp_users: BTreeMap::from([(
                "cisco".to_owned(),
                ("cisco".to_owned(), "RWDNL".to_owned()),
            )]),
            email_users: BTreeMap::new(),
            mail_domain: String::new(),
        }
    }
}

const PROCESS_NAMES: &[&str] = &[
    "DhcpServerMainProcess",
    "DnsServerProcess",
    "HttpServer",
    "HttpsServer",
    "FtpServer",
    "SmtpServer",
    "Pop3Server",
    "NtpServer",
    "SyslogServer",
    "TftpServer",
    "EmailServer",
];

pub(super) fn serves(name: &str) -> bool {
    PROCESS_NAMES.contains(&name)
}

fn text(step: &Step, index: usize) -> String {
    step.args
        .get(index)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn ip(step: &Step, index: usize) -> Ipv4Addr {
    step.args
        .get(index)
        .and_then(Value::as_ip)
        .unwrap_or(Ipv4Addr::UNSPECIFIED)
}

pub(super) fn process(
    services: &mut Services,
    name: &str,
    steps: &[Step],
) -> Result<Value, Remote> {
    let class = PROCESS_NAMES
        .iter()
        .find(|candidate| **candidate == name)
        .copied()
        .ok_or_else(|| Remote::missing("Process"))?;
    let [step, rest @ ..] = steps else {
        return Err(Remote::unknown_method(class, ""));
    };
    if step.method == "getClassName" {
        return Ok(Value::string(class));
    }
    match (class, step.method.as_str(), rest) {
        ("DhcpServerMainProcess", "getDhcpServerProcessByPortName", rest) => {
            check_args(step, class, &[TypeCode::String])?;
            dhcp(services, rest)
        }
        ("DnsServerProcess", method, []) if !matches!(method, "isEnabled" | "setEnable") => {
            dns(services, step, method)
        }
        ("HttpServer", "setPageContents", []) => {
            check_args(step, class, &[TypeCode::String, TypeCode::String])?;
            services.pages.insert(text(step, 0), text(step, 1));
            Ok(Value::Void)
        }
        ("HttpServer", "getPage", []) => {
            check_args(step, class, &[TypeCode::String])?;
            Ok(Value::string(
                services
                    .pages
                    .get(&text(step, 0))
                    .cloned()
                    .unwrap_or_default(),
            ))
        }
        ("FtpServer", "getFtpUserAccountManager", [account]) => match account.method.as_str() {
            "addFtpUser" => {
                check_args(account, "FTPUserAccountManager", &[TypeCode::String; 3])?;
                services
                    .ftp_users
                    .insert(text(account, 0), (text(account, 1), text(account, 2)));
                Ok(Value::Void)
            }
            "isExistingUser" => {
                check_args(account, "FTPUserAccountManager", &[TypeCode::String])?;
                Ok(Value::Bool(
                    services.ftp_users.contains_key(&text(account, 0)),
                ))
            }
            other => Err(Remote::unknown_method("FTPUserAccountManager", other)),
        },
        ("SmtpServer", "setServerDomainName", []) => {
            check_args(step, class, &[TypeCode::String])?;
            services.mail_domain = text(step, 0);
            Ok(Value::Void)
        }
        ("EmailServer", "addUser", []) => {
            check_args(step, class, &[TypeCode::String, TypeCode::String])?;
            let fresh = !services.email_users.contains_key(&text(step, 0));
            services.email_users.insert(text(step, 0), text(step, 1));
            Ok(Value::Bool(fresh))
        }
        (_, "isEnabled" | "isHttpsEnabled", []) => {
            no_args(step, class)?;
            services
                .enabled
                .get(class)
                .map(|on| Value::Bool(*on))
                .ok_or_else(|| Remote::unknown_method(class, &step.method))
        }
        (_, "setEnable" | "setEnabled" | "setHttpsEnable", []) => {
            check_args(step, class, &[TypeCode::Bool])?;
            let on = step.args[0].as_bool().unwrap_or_default();
            services
                .enabled
                .get_mut(class)
                .map(|state| {
                    *state = on;
                    Value::Void
                })
                .ok_or_else(|| Remote::unknown_method(class, &step.method))
        }
        (_, other, _) => Err(Remote::unknown_method(class, other)),
    }
}

fn dhcp(services: &mut Services, steps: &[Step]) -> Result<Value, Remote> {
    const CLASS: &str = "DHCPServerProcess";
    let [step, rest @ ..] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    match (step.method.as_str(), rest) {
        ("isEnable", []) => no_args(step, CLASS).map(|()| Value::Bool(services.dhcp_enabled)),
        ("setEnable", []) => {
            check_args(step, CLASS, &[TypeCode::Bool])?;
            services.dhcp_enabled = step.args[0].as_bool().unwrap_or_default();
            Ok(Value::Void)
        }
        ("getPoolCount", []) => no_args(step, CLASS)
            .map(|()| Value::Int(i32::try_from(services.pools.len()).unwrap_or_default())),
        ("addNewPool", []) => {
            check_args(
                step,
                CLASS,
                &[
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::Int,
                    TypeCode::String,
                    TypeCode::String,
                ],
            )?;
            let parse = |index| text(step, index).parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
            let (start, mask): (Ipv4Addr, Ipv4Addr) = (parse(3), parse(4));
            services.pools.push(Pool {
                name: text(step, 0),
                network: Ipv4Addr::from(u32::from(start) & u32::from(mask)),
                mask,
                gateway: parse(1),
                dns: parse(2),
                start,
                max_users: i32::try_from(step.args[5].as_i64().unwrap_or_default())
                    .unwrap_or_default(),
            });
            Ok(Value::Void)
        }
        ("getPool", rest) => {
            check_args(step, CLASS, &[TypeCode::String])?;
            let name = text(step, 0);
            let pool = services
                .pools
                .iter_mut()
                .find(|pool| pool.name == name)
                .ok_or_else(|| Remote::missing("DHCPPool"))?;
            pool_call(pool, rest)
        }
        ("getPoolAt", rest) => {
            let index = int_arg(step, CLASS)?;
            let pool = usize::try_from(index)
                .ok()
                .and_then(|index| services.pools.get_mut(index))
                .ok_or_else(|| Remote::missing("DHCPPool"))?;
            pool_call(pool, rest)
        }
        (other, _) => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn pool_call(pool: &mut Pool, steps: &[Step]) -> Result<Value, Remote> {
    const CLASS: &str = "DHCPPool";
    let [step] = steps else {
        return Err(Remote::unknown_method(CLASS, ""));
    };
    let getter = |value: Value| no_args(step, CLASS).map(|()| value);
    match step.method.as_str() {
        "getDhcpPoolName" => getter(Value::string(&pool.name)),
        "getNetworkAddress" => getter(Value::Ip(pool.network)),
        "getSubnetMask" => getter(Value::Ip(pool.mask)),
        "getDefaultRouter" => getter(Value::Ip(pool.gateway)),
        "getDnsServerIp" => getter(Value::Ip(pool.dns)),
        "getStartIp" => getter(Value::Ip(pool.start)),
        "getEndIp" => getter(Value::Ip(pool.end())),
        "getMaxUsers" => getter(Value::Int(pool.max_users)),
        "setNetworkMask" => {
            check_args(step, CLASS, &[TypeCode::Ip, TypeCode::Ip])?;
            pool.network = ip(step, 0);
            pool.mask = ip(step, 1);
            Ok(Value::Void)
        }
        "setDefaultRouter" | "setDnsServerIp" | "setStartIp" => {
            check_args(step, CLASS, &[TypeCode::Ip])?;
            let value = ip(step, 0);
            match step.method.as_str() {
                "setDefaultRouter" => pool.gateway = value,
                "setDnsServerIp" => pool.dns = value,
                _ => pool.start = value,
            }
            Ok(Value::Void)
        }
        "setMaxUsers" => {
            check_args(step, CLASS, &[TypeCode::Int])?;
            pool.max_users =
                i32::try_from(step.args[0].as_i64().unwrap_or_default()).unwrap_or_default();
            Ok(Value::Void)
        }
        other => Err(Remote::unknown_method(CLASS, other)),
    }
}

fn dns(services: &mut Services, step: &Step, method: &str) -> Result<Value, Remote> {
    const CLASS: &str = "DnsServerProcess";
    let record_type = match method {
        "addARecordToNameServerDb" => Some(1),
        "addNSRecordToNameServerDb" => Some(2),
        "addCNAMEToNameServerDb" => Some(5),
        _ => None,
    };
    if let Some(code) = record_type {
        check_args(step, CLASS, &[TypeCode::String, TypeCode::String])?;
        let record = (text(step, 0), code, text(step, 1));
        if services.records.contains(&record) {
            return Ok(Value::Bool(false));
        }
        services.records.push(record);
        return Ok(Value::Bool(true));
    }
    match method {
        "getSizeOfNameServerDb" => no_args(step, CLASS)
            .map(|()| Value::Int(i32::try_from(services.records.len()).unwrap_or_default())),
        "getRrFromNameServerDbAt" => {
            let index = int_arg(step, CLASS)?;
            let (name, code, value) = usize::try_from(index)
                .ok()
                .and_then(|index| services.records.get(index))
                .cloned()
                .ok_or_else(|| Remote::missing("DNSResourceRecord"))?;
            let (class, value) = match code {
                1 => (
                    "DnsRrA",
                    Value::Ip(value.parse().unwrap_or(Ipv4Addr::UNSPECIFIED)),
                ),
                2 => ("DnsRrNs", Value::string(value)),
                _ => ("DnsRrCname", Value::string(value)),
            };
            Ok(Value::Data {
                class: class.into(),
                fields: vec![
                    Value::string(name),
                    Value::Int(code),
                    Value::Int(1),
                    Value::Int(86_400),
                    Value::Int(4),
                    Value::qstring(""),
                    Value::Bool(false),
                    value,
                ],
            })
        }
        _ => Err(Remote::unknown_method(CLASS, method)),
    }
}
