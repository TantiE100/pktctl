use std::time::Duration;

use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{App, Listener, code_arg, deadline, text_arg};
use crate::{
    features::terminal::timeout,
    packet_tracer::{PacketTracer, PtError, expect_text},
};

const PROCESS: &str = "EmailClient";
const APP: &str = "Email app";
const SMTP: &str = "SmtpClient";
const POP3: &str = "Pop3Client";
const MAIL_SENT: &str = "mailSent";
const MAIL_RECEIVED: &str = "mailReceived";
const RECEIVE_FAILED: &str = "errorReceivingMail";
const SMTP_SUCCESS: i64 = 2;
const DEFAULT_RECEIVE_SECS: u64 = 10;
const MORE_MAIL_GRACE: Duration = Duration::from_secs(2);

// SmtpResponseType and Pop3ResponseType values, as the events report them.
const SMTP_STATUSES: &[(i64, &str)] = &[
    (4, "recipient does not exist"),
    (5, "server error"),
    (6, "timeout"),
    (7, "connection reset"),
    (8, "DNS server not found"),
    (9, "server name not resolved"),
    (10, "protocol error"),
    (11, "user not found"),
    (12, "wrong server domain"),
    (13, "server not found"),
];
const POP3_STATUSES: &[(i64, &str)] = &[
    (3, "server error"),
    (4, "timeout"),
    (5, "connection reset"),
    (6, "DNS server not found"),
    (7, "server name not resolved"),
    (8, "protocol error"),
    (9, "wrong user name or password"),
];

const FIELDS: [(&str, &str); 6] = [
    ("Name", "name"),
    ("MailId", "email"),
    ("User", "username"),
    ("Password", "password"),
    ("Pop3Server", "incoming_server"),
    ("SmtpServer", "outgoing_server"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EmailAccountRequest {
    /// PC, laptop or server whose Email app to set up.
    pub device: String,
    /// Your Name, for example `Ana`.
    pub name: String,
    /// Email Address, for example `ana@gamc.bo`.
    pub email: String,
    /// User Name on the mail server, usually the part before `@`.
    pub username: String,
    pub password: String,
    /// Incoming Mail Server (POP3): address or name.
    pub incoming_server: String,
    /// Outgoing Mail Server (SMTP): address or name.
    pub outgoing_server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EmailAccount {
    pub device: String,
    pub name: String,
    pub email: String,
    pub username: String,
    pub incoming_server: String,
    pub outgoing_server: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SendRequest {
    /// Device whose Email app sends, set up with `configure_email`.
    pub device: String,
    /// Recipient address, for example `admin@gamc.bo`.
    pub to: String,
    pub subject: String,
    pub body: String,
    /// Seconds to wait for the server. Defaults to 30, maximum 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Sent {
    pub device: String,
    pub from: String,
    pub to: String,
    pub subject: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReceiveRequest {
    /// Device whose Email app receives, set up with `configure_email`.
    pub device: String,
    /// Seconds to wait for mail. Defaults to 10; an empty mailbox answers after this long.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ReceivedMail {
    pub from: String,
    pub subject: String,
    pub date: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Received {
    pub device: String,
    pub mails: Vec<ReceivedMail>,
}

struct Account {
    values: Vec<String>,
}

impl Account {
    fn get(&self, field: &str) -> &str {
        FIELDS
            .iter()
            .position(|(_, name)| *name == field)
            .and_then(|index| self.values.get(index))
            .map_or("", String::as_str)
    }
}

pub async fn configure_email<P: PacketTracer>(
    packet_tracer: &P,
    request: &EmailAccountRequest,
) -> Result<EmailAccount, PtError> {
    let given = [
        &request.name,
        &request.email,
        &request.username,
        &request.password,
        &request.incoming_server,
        &request.outgoing_server,
    ];
    for ((_, field), value) in FIELDS.iter().zip(given) {
        if value.trim().is_empty() {
            return Err(PtError::InvalidInput(format!("{field} is required")));
        }
    }
    if !request.email.contains('@') {
        return Err(PtError::InvalidInput(format!(
            "`{}` is not an email address",
            request.email
        )));
    }
    let app = App::open(packet_tracer, &request.device, PROCESS, APP).await?;
    for ((suffix, _), value) in FIELDS.iter().zip(given) {
        app.call(user(&app).method(format!("set{suffix}"), [Value::string(value.trim())]))
            .await?;
    }
    let account = read_account(&app).await?;
    Ok(EmailAccount {
        device: app.device,
        name: account.get("name").to_owned(),
        email: account.get("email").to_owned(),
        username: account.get("username").to_owned(),
        incoming_server: account.get("incoming_server").to_owned(),
        outgoing_server: account.get("outgoing_server").to_owned(),
    })
}

pub async fn send_email<P: PacketTracer>(
    packet_tracer: &P,
    request: &SendRequest,
) -> Result<Sent, PtError> {
    let wait = timeout(request.timeout_secs)?;
    let to = request.to.trim();
    if !to.contains('@') {
        return Err(PtError::InvalidInput(format!(
            "`{to}` is not an email address"
        )));
    }
    let app = App::open(packet_tracer, &request.device, PROCESS, APP).await?;
    let account = configured(&app).await?;
    let smtp = app.at("getSmtpClient", []);
    let object = app.uuid_of(smtp.clone()).await?;
    let mut listener = Listener::start(packet_tracer, SMTP, &object, &[MAIL_SENT]).await?;
    let outcome = async {
        app.call(smtp.method(
            "sendMail",
            [
                Value::string(account.get("email")),
                Value::string(to),
                Value::string(&request.subject),
                Value::qstring(&request.body),
                Value::string(account.get("password")),
                Value::string(account.get("outgoing_server")),
            ],
        ))
        .await?;
        listener.next(deadline(wait)).await
    }
    .await;
    listener.stop().await;
    let sent = outcome?.ok_or_else(|| {
        PtError::Rejected(format!(
            "the SMTP server {} did not answer within {} seconds",
            account.get("outgoing_server"),
            wait.as_secs()
        ))
    })?;
    let code = code_arg(&sent, 3);
    if code != SMTP_SUCCESS {
        return Err(PtError::Rejected(format!(
            "the mail was not sent: {}; if it was a timeout, the first mail after cabling can \
             lose the race with ARP, so send it again",
            describe_code(SMTP_STATUSES, code)
        )));
    }
    Ok(Sent {
        device: app.device,
        from: account.get("email").to_owned(),
        to: to.to_owned(),
        subject: request.subject.clone(),
    })
}

pub async fn receive_email<P: PacketTracer>(
    packet_tracer: &P,
    request: &ReceiveRequest,
) -> Result<Received, PtError> {
    let wait = timeout(Some(request.timeout_secs.unwrap_or(DEFAULT_RECEIVE_SECS)))?;
    let app = App::open(packet_tracer, &request.device, PROCESS, APP).await?;
    configured(&app).await?;
    let pop3 = app.at("getPop3Client", []);
    let object = app.uuid_of(pop3.clone()).await?;
    let mut listener = Listener::start(
        packet_tracer,
        POP3,
        &object,
        &[MAIL_RECEIVED, RECEIVE_FAILED],
    )
    .await?;
    let outcome = async {
        app.call(pop3.method("getMailIpc", [])).await?;
        let mut mails = Vec::new();
        let mut until = deadline(wait);
        while let Some(event) = listener.next(until).await? {
            if event.name == RECEIVE_FAILED {
                return Err(PtError::Rejected(format!(
                    "the mail could not be received: {}",
                    describe_code(POP3_STATUSES, code_arg(&event, 0))
                )));
            }
            if event.name == MAIL_RECEIVED {
                mails.push(ReceivedMail {
                    from: text_arg(&event, 0),
                    subject: text_arg(&event, 1),
                    date: text_arg(&event, 2),
                    body: text_arg(&event, 3),
                });
                until = deadline(MORE_MAIL_GRACE);
            }
        }
        Ok(mails)
    }
    .await;
    listener.stop().await;
    Ok(Received {
        device: app.device,
        mails: outcome?,
    })
}

fn user<P>(app: &App<'_, P>) -> Call {
    app.process.clone().method("getEmailUser", [])
}

async fn read_account<P: PacketTracer>(app: &App<'_, P>) -> Result<Account, PtError> {
    let mut values = Vec::with_capacity(FIELDS.len());
    for (suffix, field) in FIELDS {
        let value = app
            .call(user(app).method(format!("get{suffix}"), []))
            .await?;
        values.push(expect_text(&value, field)?);
    }
    Ok(Account { values })
}

async fn configured<P: PacketTracer>(app: &App<'_, P>) -> Result<Account, PtError> {
    let account = read_account(app).await?;
    let missing: Vec<&str> = FIELDS
        .iter()
        .map(|(_, field)| *field)
        .filter(|field| *field != "name" && account.get(field).trim().is_empty())
        .collect();
    if !missing.is_empty() {
        return Err(PtError::InvalidInput(format!(
            "the Email app of `{}` has no {}; set it up with configure_email first",
            app.device,
            missing.join(", ")
        )));
    }
    Ok(account)
}

fn describe_code(table: &[(i64, &str)], code: i64) -> String {
    table
        .iter()
        .find(|(value, _)| *value == code)
        .map_or_else(|| format!("status {code}"), |(_, name)| (*name).to_owned())
}
