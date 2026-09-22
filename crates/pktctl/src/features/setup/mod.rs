use std::path::{Path, PathBuf};

use rmcp::{Json, tool, tool_router};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    config::SetupSettings,
    packet_tracer::{PacketTracer, PtError},
    server::PktctlServer,
};

const TEMPLATE: &str = include_str!("../../../../../docs/features/pktctl-exapp.xml");
const TEMPLATE_ID: &str = "<ID>dev.pktctl</ID>";
const TEMPLATE_KEY: &str = "<KEY>REPLACE_WITH_YOUR_SECRET</KEY>";
const PTA_FILE: &str = "pktctl.pta";
const XML_FILE: &str = "pktctl-exapp.xml";
const INSTALL_PREFIX: &str = "Cisco Packet Tracer";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SetupResult {
    pub app_id: String,
    pub pta: String,
    pub meta_tool: String,
    pub steps: Vec<String>,
}

pub async fn generate(settings: &SetupSettings) -> Result<SetupResult, PtError> {
    let xml = render(&settings.credentials.app_id, &settings.credentials.secret)?;
    let meta = find_meta(settings.packet_tracer_home.as_deref())?;

    std::fs::create_dir_all(&settings.output_dir).map_err(|error| {
        PtError::InvalidInput(format!(
            "could not create {}: {error}",
            settings.output_dir.display()
        ))
    })?;
    let xml_path = settings.output_dir.join(XML_FILE);
    let pta_path = settings.output_dir.join(PTA_FILE);
    write_private(&xml_path, &xml)?;

    let outcome = tokio::process::Command::new(&meta)
        .arg(&pta_path)
        .arg(&xml_path)
        .output()
        .await;
    let _ = std::fs::remove_file(&xml_path);

    let output = outcome.map_err(|error| {
        PtError::Transport(format!("could not run {}: {error}", meta.display()))
    })?;
    if !output.status.success() || !pta_path.exists() {
        return Err(PtError::Transport(format!(
            "{} did not produce {}: {}",
            meta.display(),
            pta_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let pta = pta_path.display().to_string();
    Ok(SetupResult {
        app_id: settings.credentials.app_id.clone(),
        steps: vec![
            "Open Packet Tracer and choose Extensions > IPC > Configure Apps.".into(),
            format!("Click Add and select {pta}."),
            "Click Ok and call status to confirm. Quit Packet Tracer normally once (Cmd+Q or \
             File > Exit) so it saves the registration; a crash or forced quit loses it."
                .into(),
        ],
        meta_tool: meta.display().to_string(),
        pta,
    })
}

fn render(app_id: &str, secret: &str) -> Result<String, PtError> {
    for (field, value) in [("PKTCTL_APP_ID", app_id), ("PKTCTL_SECRET", secret)] {
        let safe = !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
        if !safe {
            return Err(PtError::InvalidInput(format!(
                "{field} may only contain letters, digits, dots, dashes and underscores"
            )));
        }
    }
    Ok(TEMPLATE
        .replace(TEMPLATE_ID, &format!("<ID>{app_id}</ID>"))
        .replace(TEMPLATE_KEY, &format!("<KEY>{secret}</KEY>")))
}

fn find_meta(home: Option<&Path>) -> Result<PathBuf, PtError> {
    let candidates: Vec<PathBuf> = match home {
        Some(home) => meta_in(home),
        None => default_installs()
            .iter()
            .flat_map(|home| meta_in(home))
            .collect(),
    };
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            PtError::InvalidInput(
                "could not find Packet Tracer's `meta` tool; set PKTCTL_PT_HOME to the Packet \
                 Tracer installation folder"
                    .into(),
            )
        })
}

fn meta_in(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![home.to_path_buf(), home.join("Contents")];
    if let Ok(entries) = std::fs::read_dir(home) {
        let mut bundles: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
            .collect();
        bundles.sort();
        roots.extend(bundles.into_iter().map(|bundle| bundle.join("Contents")));
    }
    roots
        .into_iter()
        .flat_map(|root| {
            let extensions = root.join("extensions");
            [extensions.join("meta"), extensions.join("meta.exe")]
        })
        .collect()
}

fn default_installs() -> Vec<PathBuf> {
    let mut installs = Vec::new();
    for parent in ["/Applications", "C:\\Program Files", "/opt"] {
        let Ok(entries) = std::fs::read_dir(parent) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(INSTALL_PREFIX) || name == "pt")
            })
            .collect();
        found.sort_by(|a, b| b.cmp(a));
        installs.extend(found);
    }
    installs
}

fn write_private(path: &Path, contents: &str) -> Result<(), PtError> {
    let failed = |error: std::io::Error| {
        PtError::InvalidInput(format!("could not write {}: {error}", path.display()))
    };
    std::fs::write(path, contents).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(failed)?;
    }
    Ok(())
}

#[tool_router(router = setup_router, vis = "pub(crate)")]
impl<P: PacketTracer> PktctlServer<P> {
    #[tool(
        name = "setup_exapp",
        description = "Create the Packet Tracer app registration file (.pta) for pktctl's \
                       configured app id and secret, using Packet Tracer's own `meta` tool, and \
                       return the one-time steps to register it. Works while Packet Tracer \
                       still rejects pktctl.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn setup_exapp_tool(&self) -> Result<Json<SetupResult>, String> {
        generate(self.setup_settings())
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use ptmp::Credentials;

    use super::*;

    fn settings(output_dir: PathBuf, home: Option<PathBuf>) -> SetupSettings {
        SetupSettings {
            credentials: Credentials {
                app_id: "dev.pktctl".into(),
                secret: "0a1b2c3d".into(),
            },
            packet_tracer_home: home,
            output_dir,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("pktctl-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn renders_the_template_with_all_privileges() {
        let xml = render("dev.lab", "s3cr3t").unwrap();
        assert!(xml.contains("<ID>dev.lab</ID>"));
        assert!(xml.contains("<KEY>s3cr3t</KEY>"));
        assert!(!xml.contains("REPLACE_WITH_YOUR_SECRET"));
        assert_eq!(xml.matches("<PRIVILEGE>").count(), 11);
        for privilege in ["FILE", "IPC", "CHANGE_NETWORK_INFO"] {
            assert!(xml.contains(&format!("<PRIVILEGE>{privilege}</PRIVILEGE>")));
        }
    }

    #[test]
    fn refuses_values_that_would_break_the_xml() {
        assert!(render("dev.lab", "a<b").is_err());
        assert!(render("dev lab", "secret").is_err());
        assert!(render("", "secret").is_err());
    }

    #[test]
    fn finds_meta_inside_a_macos_style_install() {
        let home = scratch("find");
        let extensions = home.join("Cisco Packet Tracer 9.0.1.app/Contents/extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        std::fs::write(extensions.join("meta"), "").unwrap();
        assert_eq!(find_meta(Some(&home)).unwrap(), extensions.join("meta"));
        assert!(find_meta(Some(&home.join("missing"))).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_meta_and_leaves_no_plain_secret_behind() {
        use std::os::unix::fs::PermissionsExt;

        let home = scratch("home");
        let output = scratch("output");
        let extensions = home.join("extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        let meta = extensions.join("meta");
        std::fs::write(&meta, "#!/bin/sh\ncp \"$2\" \"$1\"\n").unwrap();
        std::fs::set_permissions(&meta, std::fs::Permissions::from_mode(0o755)).unwrap();

        let result = generate(&settings(output.clone(), Some(home.clone())))
            .await
            .unwrap();
        assert_eq!(result.pta, output.join(PTA_FILE).display().to_string());
        assert!(
            std::fs::read_to_string(&result.pta)
                .unwrap()
                .contains("<ID>dev.pktctl</ID>")
        );
        assert!(!output.join(XML_FILE).exists());
        assert_eq!(result.steps.len(), 3);

        std::fs::remove_dir_all(home).unwrap();
        std::fs::remove_dir_all(output).unwrap();
    }
}
