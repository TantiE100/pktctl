use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Dhcp,
    Dns,
    Http,
    Https,
    Ftp,
    Smtp,
    Pop3,
    Ntp,
    Syslog,
    Tftp,
}

pub(crate) struct ServiceSpec {
    pub service: Service,
    pub process: &'static str,
    pub getter: &'static str,
    pub setter: &'static str,
}

pub(crate) const SERVICES: &[ServiceSpec] = &[
    spec(Service::Dns, "DnsServerProcess", "isEnabled", "setEnable"),
    spec(Service::Http, "HttpServer", "isEnabled", "setEnable"),
    spec(
        Service::Https,
        "HttpsServer",
        "isHttpsEnabled",
        "setHttpsEnable",
    ),
    spec(Service::Ftp, "FtpServer", "isEnabled", "setEnabled"),
    spec(Service::Smtp, "SmtpServer", "isEnabled", "setEnable"),
    spec(Service::Pop3, "Pop3Server", "isEnabled", "setEnable"),
    spec(Service::Ntp, "NtpServer", "isEnabled", "setEnabled"),
    spec(Service::Syslog, "SyslogServer", "isEnabled", "setEnable"),
    spec(Service::Tftp, "TftpServer", "isEnabled", "setEnabled"),
];

const fn spec(
    service: Service,
    process: &'static str,
    getter: &'static str,
    setter: &'static str,
) -> ServiceSpec {
    ServiceSpec {
        service,
        process,
        getter,
        setter,
    }
}

pub(crate) const DHCP_PROCESS: &str = "DhcpServerMainProcess";
