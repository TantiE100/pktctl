use std::{fmt, sync::Arc};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler,
};

use crate::packet_tracer::PacketTracer;

const INSTRUCTIONS: &str = "pktctl drives a running Cisco Packet Tracer over its native IPC \
protocol. Call `status` first: it reports whether Packet Tracer is reachable and why not. \
Use `list_devices` to discover device names before targeting them, and `run_cli` to execute \
IOS commands on routers and switches.";

pub struct PktctlServer<P> {
    packet_tracer: Arc<P>,
    tool_router: ToolRouter<Self>,
}

impl<P: PacketTracer> PktctlServer<P> {
    pub fn new(packet_tracer: P) -> Self {
        Self {
            packet_tracer: Arc::new(packet_tracer),
            tool_router: Self::status_router()
                + Self::devices_router()
                + Self::cli_router()
                + Self::host_console_router()
                + Self::catalog_router()
                + Self::links_router()
                + Self::hosts_router(),
        }
    }

    pub(crate) fn packet_tracer(&self) -> &P {
        &self.packet_tracer
    }

    pub async fn serve_stdio(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    }
}

impl<P> Clone for PktctlServer<P> {
    fn clone(&self) -> Self {
        Self {
            packet_tracer: Arc::clone(&self.packet_tracer),
            tool_router: self.tool_router.clone(),
        }
    }
}

impl<P> fmt::Debug for PktctlServer<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PktctlServer").finish_non_exhaustive()
    }
}

#[tool_handler(router = self.tool_router)]
// The rmcp macro expands to async fns that only delegate, which trips this lint.
#[allow(clippy::unused_async_trait_impl)]
impl<P: PacketTracer> ServerHandler for PktctlServer<P> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("pktctl", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
