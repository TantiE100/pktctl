use ptmp::Value;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{App, Listener, code_arg, deadline, text_arg};
use crate::{
    features::{activity::html_to_text, terminal::timeout},
    packet_tracer::{PacketTracer, PtError},
};

const PROCESS: &str = "HttpClient";
const DONE: &str = "onDone";

// HTTPType values that onDone reports.
const STATUSES: &[(i64, &str)] = &[
    (3, "ok"),
    (4, "unauthorized"),
    (5, "not_found"),
    (6, "invalid"),
    (7, "timeout"),
    (8, "connection_reset"),
    (9, "dns_server_not_found"),
    (10, "host_not_found"),
    (11, "protocol_error"),
    (12, "login_failed"),
    (13, "connection_closed"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BrowseRequest {
    /// PC, laptop or server whose Web Browser opens the page.
    pub device: String,
    /// Address as typed in the browser, for example `http://www.gamc.bo` or
    /// `https://192.168.10.10/index.html`. `http://` is added when missing.
    pub url: String,
    /// Seconds to wait for the page. Defaults to 30, maximum 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Page {
    pub device: String,
    pub url: String,
    /// `ok`, `not_found`, `timeout`, `host_not_found`, `dns_server_not_found`, ...
    pub status: String,
    /// The address the name resolved to, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub html: String,
    /// The page as readable text.
    pub text: String,
}

pub async fn browse_web<P: PacketTracer>(
    packet_tracer: &P,
    request: &BrowseRequest,
) -> Result<Page, PtError> {
    let url = normalize(&request.url)?;
    let wait = timeout(request.timeout_secs)?;
    let app = App::open(packet_tracer, &request.device, PROCESS, "Web Browser").await?;
    app.call(app.at("setHttps", [Value::Bool(url.starts_with("https://"))]))
        .await?;
    let object = app.uuid_of(app.process.clone()).await?;
    let mut listener = Listener::start(packet_tracer, PROCESS, &object, &[DONE]).await?;
    let outcome = async {
        app.call(app.at("go", [Value::string(&url)])).await?;
        let until = deadline(wait);
        while let Some(event) = listener.next(until).await? {
            if event.name == DONE {
                return Ok(Some(event));
            }
        }
        Ok::<_, PtError>(None)
    }
    .await;
    listener.stop().await;
    let Some(done) = outcome? else {
        app.call(app.at("cancel", [])).await?;
        return Err(PtError::Rejected(format!(
            "no answer from {url} within {} seconds",
            wait.as_secs()
        )));
    };

    let code = code_arg(&done, 2);
    let status = STATUSES
        .iter()
        .find(|(value, _)| *value == code)
        .map_or_else(|| format!("code_{code}"), |(_, name)| (*name).to_owned());
    let server =
        Some(text_arg(&done, 1)).filter(|address| !address.is_empty() && address != "0.0.0.0");
    let html = text_arg(&done, 3);
    Ok(Page {
        device: app.device,
        url,
        status,
        server,
        text: html_to_text(&html),
        html,
    })
}

fn normalize(url: &str) -> Result<String, PtError> {
    let url = url.trim();
    if url.is_empty() || url.contains(char::is_whitespace) {
        return Err(PtError::InvalidInput(
            "url must be an address such as http://www.gamc.bo".into(),
        ));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url.to_owned())
    } else if url.contains("://") {
        Err(PtError::InvalidInput(format!(
            "`{url}`: the Web Browser only speaks http and https"
        )))
    } else {
        Ok(format!("http://{url}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_the_scheme_the_browser_assumes() {
        assert_eq!(normalize("www.gamc.bo").unwrap(), "http://www.gamc.bo");
        assert_eq!(
            normalize(" https://10.0.0.1/a.html ").unwrap(),
            "https://10.0.0.1/a.html"
        );
        assert!(normalize("ftp://10.0.0.1").is_err());
        assert!(normalize("").is_err());
    }
}
