use std::{collections::BTreeMap, net::Ipv4Addr};

use ptmp::{Event, Step, TypeCode, Value};

use super::{
    State,
    remote::{Remote, check_args, int_arg, no_args, string_arg},
    services::StoredMail,
};

const HTTP: &str = "HttpClient";
const EMAIL: &str = "EmailClient";
const SMTP: &str = "SmtpClient";
const POP3: &str = "Pop3Client";
const FILES: &str = "FileManager";
const VPN: &str = "EasyVpnClient";
const TUNNEL_ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 50, 0, 10);
const DIRECTORY: &str = "Directory";
const FILE: &str = "SimFile";
const ROOT: &str = "c:/";
const HTTP_OK: i32 = 3;
const HTTP_NOT_FOUND: i32 = 5;
const HTTP_TIMEOUT: i32 = 7;
const HTTP_UNRESOLVED: i32 = 10;
const SMTP_SUCCESS: i32 = 2;
const SMTP_TIMEOUT: i32 = 6;
const POP3_TIMEOUT: i32 = 4;
const POP3_USER_NOT_FOUND: i32 = 9;
const MAIL_DATE: &str = "09.22.2026 12:00:00.000 PM";

#[derive(Debug, Clone, Default)]
struct Account {
    name: String,
    mail_id: String,
    user: String,
    password: String,
    smtp: String,
    pop3: String,
}

/// The Desktop apps of an end device: web browser, email client and text files.
#[derive(Debug, Clone, Default)]
struct Vpn {
    server: Option<Ipv4Addr>,
    fields: BTreeMap<String, String>,
    connected: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Desktop {
    last_page: String,
    account: Account,
    files: BTreeMap<String, String>,
    vpn: Vpn,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            last_page: String::new(),
            account: Account::default(),
            vpn: Vpn::default(),
            files: BTreeMap::from([(
                "sampleFile.txt".to_owned(),
                "This is a sample text file".to_owned(),
            )]),
        }
    }
}

pub(super) fn serves(name: &str) -> bool {
    matches!(name, HTTP | EMAIL | FILES | VPN)
}

pub(super) fn process(
    state: &mut State,
    index: usize,
    name: &str,
    steps: &[Step],
) -> Result<Value, Remote> {
    let class = match name {
        HTTP => HTTP,
        EMAIL => EMAIL,
        VPN => VPN,
        _ => FILES,
    };
    match steps {
        [step] if step.method == "getClassName" => Ok(Value::string(class)),
        [step] if step.method == "getObjectUuid" => Ok(Value::Uuid(uuid(index, class))),
        _ => match class {
            HTTP => http(state, index, steps),
            EMAIL => email(state, index, steps),
            VPN => vpn(state, index, steps),
            _ => files(own(state, index)?, steps),
        },
    }
}

fn own(state: &mut State, index: usize) -> Result<&mut Desktop, Remote> {
    state.devices[index]
        .desktop
        .as_mut()
        .ok_or_else(|| Remote::missing("Process"))
}

fn uuid(index: usize, class: &str) -> String {
    format!("{{desktop-{index}-{class}}}")
}

fn event(index: usize, class: &str, name: &str, args: Vec<Value>) -> Event {
    Event {
        token: "canvas".into(),
        class: class.into(),
        object_uuid: uuid(index, class),
        name: name.into(),
        args,
    }
}

fn http(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    let [step] = steps else {
        return Err(Remote::unknown_method(HTTP, ""));
    };
    match step.method.as_str() {
        "setHttps" => check_args(step, HTTP, &[TypeCode::Bool]).map(|()| Value::Void),
        "cancel" => no_args(step, HTTP).map(|()| Value::Bool(true)),
        "getLastPageContent" => {
            no_args(step, HTTP)?;
            Ok(Value::string(&own(state, index)?.last_page))
        }
        "go" => {
            let url = string_arg(step, HTTP)?.to_owned();
            let (code, address, page) = fetch(state, index, &url);
            own(state, index)?.last_page.clone_from(&page);
            state.events.extend([
                event(index, HTTP, "onStart", vec![Value::string(&url)]),
                event(
                    index,
                    HTTP,
                    "onDone",
                    vec![
                        Value::string(&url),
                        Value::Ip(address),
                        Value::Int(code),
                        Value::string(page),
                    ],
                ),
            ]);
            Ok(Value::Bool(true))
        }
        other => Err(Remote::unknown_method(HTTP, other)),
    }
}

fn fetch(state: &State, index: usize, url: &str) -> (i32, Ipv4Addr, String) {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(address) = host.parse().ok().or_else(|| resolve(state, index, host)) else {
        return (HTTP_UNRESOLVED, Ipv4Addr::UNSPECIFIED, String::new());
    };
    match server_at(state, address).and_then(|server| state.devices[server].services.as_ref()) {
        None => (HTTP_TIMEOUT, address, String::new()),
        Some(services) => services.page(path).map_or_else(
            || (HTTP_NOT_FOUND, address, String::new()),
            |page| (HTTP_OK, address, page.to_owned()),
        ),
    }
}

fn resolve(state: &State, index: usize, name: &str) -> Option<Ipv4Addr> {
    let dns = state.devices[index]
        .ports
        .iter()
        .map(|port| port.dns)
        .find(|dns| !dns.is_unspecified())?;
    state.devices[server_at(state, dns)?]
        .services
        .as_ref()?
        .resolve(name)
}

fn server_at(state: &State, address: Ipv4Addr) -> Option<usize> {
    state
        .devices
        .iter()
        .position(|device| device.ports.iter().any(|port| port.ip == address))
}

fn email(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    match steps {
        [user, field] if user.method == "getEmailUser" => {
            no_args(user, EMAIL)?;
            account_field(&mut own(state, index)?.account, field)
        }
        [client, step] if client.method == "getSmtpClient" && step.method == "getObjectUuid" => {
            Ok(Value::Uuid(uuid(index, SMTP)))
        }
        [client, step] if client.method == "getPop3Client" && step.method == "getObjectUuid" => {
            Ok(Value::Uuid(uuid(index, POP3)))
        }
        [client, step] if client.method == "getSmtpClient" && step.method == "sendMail" => {
            send_mail(state, index, step)
        }
        [client, step] if client.method == "getPop3Client" && step.method == "getMailIpc" => {
            no_args(step, POP3)?;
            receive_mail(state, index)
        }
        [other, ..] => Err(Remote::unknown_method(EMAIL, &other.method)),
        [] => Err(Remote::unknown_method(EMAIL, "")),
    }
}

fn send_mail(state: &mut State, index: usize, step: &Step) -> Result<Value, Remote> {
    check_args(
        step,
        SMTP,
        &[
            TypeCode::String,
            TypeCode::String,
            TypeCode::String,
            TypeCode::QString,
            TypeCode::String,
            TypeCode::String,
        ],
    )?;
    let text = |at: usize| step.args[at].as_str().unwrap_or_default().to_owned();
    let (from, to, subject, body, server) = (text(0), text(1), text(2), text(3), text(5));
    let mail = StoredMail {
        from,
        subject: subject.clone(),
        body: body.clone(),
    };
    let delivered = server
        .parse()
        .ok()
        .and_then(|address| server_at(state, address))
        .and_then(|server| state.devices[server].services.as_mut())
        .and_then(|services| services.deliver(&to, mail));
    let code = if delivered.is_some() {
        SMTP_SUCCESS
    } else {
        SMTP_TIMEOUT
    };
    state.events.push(event(
        index,
        SMTP,
        "mailSent",
        vec![
            Value::string(to),
            Value::string(subject),
            Value::string(body),
            Value::Int(code),
        ],
    ));
    Ok(Value::Bool(true))
}

fn receive_mail(state: &mut State, index: usize) -> Result<Value, Remote> {
    let account = own(state, index)?.account.clone();
    let services = account
        .pop3
        .parse()
        .ok()
        .and_then(|address| server_at(state, address))
        .and_then(|server| state.devices[server].services.as_mut());
    let failed = |code| event(index, POP3, "errorReceivingMail", vec![Value::Int(code)]);
    let events = match services.map(|services| services.collect(&account.user, &account.password)) {
        None => vec![failed(POP3_TIMEOUT)],
        Some(None) => vec![failed(POP3_USER_NOT_FOUND)],
        Some(Some(mails)) => mails
            .into_iter()
            .map(|mail| {
                event(
                    index,
                    POP3,
                    "mailReceived",
                    vec![
                        Value::string(mail.from),
                        Value::string(mail.subject),
                        Value::qstring(MAIL_DATE),
                        Value::string(mail.body),
                    ],
                )
            })
            .collect(),
    };
    state.events.extend(events);
    Ok(Value::Bool(true))
}

/// Connects when a device answers at the server address and every field is filled in;
/// the canvas does not model IKE.
fn vpn(state: &mut State, index: usize, steps: &[Step]) -> Result<Value, Remote> {
    let [step] = steps else {
        return Err(Remote::unknown_method(VPN, ""));
    };
    let reachable = own(state, index)?
        .vpn
        .server
        .is_some_and(|server| server_at(state, server).is_some());
    let vpn = &mut own(state, index)?.vpn;
    match step.method.as_str() {
        "setServerIp" => {
            check_args(step, VPN, &[TypeCode::Ip])?;
            vpn.server = step.args[0].as_ip();
            Ok(Value::Void)
        }
        setter @ ("setGroupName" | "setGroupKey" | "setUsername" | "setPassword") => {
            let value = string_arg(step, VPN)?.to_owned();
            vpn.fields.insert(setter.to_owned(), value);
            Ok(Value::Void)
        }
        "connect" => {
            no_args(step, VPN)?;
            vpn.connected = reachable && vpn.fields.len() == 4;
            Ok(Value::Void)
        }
        "disconnect" => {
            no_args(step, VPN)?;
            vpn.connected = false;
            Ok(Value::Void)
        }
        "isConnected" => no_args(step, VPN).map(|()| Value::Bool(vpn.connected)),
        "getServerIp" => {
            no_args(step, VPN).map(|()| Value::Ip(vpn.server.unwrap_or(Ipv4Addr::UNSPECIFIED)))
        }
        getter @ ("getGroupName" | "getUsername") => {
            no_args(step, VPN)?;
            let setter = getter.replacen("get", "set", 1);
            Ok(Value::string(
                vpn.fields.get(&setter).cloned().unwrap_or_default(),
            ))
        }
        "getTunnelIp" => no_args(step, VPN).map(|()| {
            Value::Ip(if vpn.connected {
                TUNNEL_ADDRESS
            } else {
                Ipv4Addr::UNSPECIFIED
            })
        }),
        other => Err(Remote::unknown_method(VPN, other)),
    }
}

fn account_field(account: &mut Account, step: &Step) -> Result<Value, Remote> {
    const USER: &str = "EmailUser";
    let slot = match step
        .method
        .trim_start_matches("get")
        .trim_start_matches("set")
    {
        "Name" => &mut account.name,
        "MailId" => &mut account.mail_id,
        "User" => &mut account.user,
        "Password" => &mut account.password,
        "SmtpServer" => &mut account.smtp,
        "Pop3Server" => &mut account.pop3,
        _ => return Err(Remote::unknown_method(USER, &step.method)),
    };
    if step.method.starts_with("set") {
        string_arg(step, USER)?.clone_into(slot);
        Ok(Value::Void)
    } else {
        no_args(step, USER).map(|()| Value::string(slot.as_str()))
    }
}

fn files(desktop: &mut Desktop, steps: &[Step]) -> Result<Value, Remote> {
    let [directory, rest @ ..] = steps else {
        return Err(Remote::unknown_method(FILES, ""));
    };
    if directory.method != "getDirectory" {
        return Err(Remote::unknown_method(FILES, &directory.method));
    }
    check_args(directory, FILES, &[TypeCode::String, TypeCode::Bool])?;
    if directory.args[0].as_str() != Some(ROOT) {
        return Err(Remote::missing(DIRECTORY));
    }
    let files = &mut desktop.files;
    let text = |step: &Step, at: usize| step.args[at].as_str().unwrap_or_default().to_owned();
    match rest {
        [step] => match step.method.as_str() {
            "getFileCount" => no_args(step, DIRECTORY)
                .map(|()| Value::Int(i32::try_from(files.len()).unwrap_or(i32::MAX))),
            "fileExist" => {
                check_args(step, DIRECTORY, &[TypeCode::String])?;
                Ok(Value::Bool(files.contains_key(&text(step, 0))))
            }
            "addTextFile" => {
                check_args(
                    step,
                    DIRECTORY,
                    &[TypeCode::String, TypeCode::String, TypeCode::Bool],
                )?;
                if files.contains_key(&text(step, 0)) {
                    return Ok(Value::Bool(false));
                }
                files.insert(text(step, 0), text(step, 1));
                Ok(Value::Bool(true))
            }
            "removeFile" => {
                check_args(step, DIRECTORY, &[TypeCode::String, TypeCode::Bool])?;
                Ok(Value::Bool(files.remove(&text(step, 0)).is_some()))
            }
            other => Err(Remote::unknown_method(DIRECTORY, other)),
        },
        [pick, step] => {
            let name = match pick.method.as_str() {
                "getFileAt" => {
                    let at = int_arg(pick, DIRECTORY)?;
                    usize::try_from(at)
                        .ok()
                        .and_then(|at| files.keys().nth(at).cloned())
                }
                "getFile" => {
                    check_args(pick, DIRECTORY, &[TypeCode::String])?;
                    Some(text(pick, 0)).filter(|name| files.contains_key(name))
                }
                other => return Err(Remote::unknown_method(DIRECTORY, other)),
            }
            .ok_or_else(|| Remote::missing(FILE))?;
            file(files, &name, step)
        }
        _ => Err(Remote::unknown_method(DIRECTORY, "")),
    }
}

fn file(files: &mut BTreeMap<String, String>, name: &str, step: &Step) -> Result<Value, Remote> {
    let contents = files.get_mut(name).ok_or_else(|| Remote::missing(FILE))?;
    match step.method.as_str() {
        "getName" => no_args(step, FILE).map(|()| Value::string(name)),
        "getSize" => no_args(step, FILE)
            .map(|()| Value::Int(i32::try_from(contents.len()).unwrap_or(i32::MAX))),
        "getContent" => {
            check_args(step, FILE, &[TypeCode::Bool])?;
            Ok(Value::Data {
                class: "TextFileContent".into(),
                fields: vec![Value::string(contents.as_str())],
            })
        }
        "setTextContent" => {
            check_args(step, FILE, &[TypeCode::String, TypeCode::Bool])?;
            step.args[0]
                .as_str()
                .unwrap_or_default()
                .clone_into(contents);
            Ok(Value::Void)
        }
        other => Err(Remote::unknown_method(FILE, other)),
    }
}
