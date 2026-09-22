use futures::future::try_join_all;
use ptmp::{Call, Value};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::packet_tracer::{
    PacketTracer, PtError, api::ApiIndex, expect_bool, expect_integer, expect_text,
};

const TRAFFIC_ENUM: &str = "TrafficType";
const TRAFFIC_PREFIX: &str = "TRAFFIC_TYPE_";
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;
const FLAGS: &[(&str, &str)] = &[
    ("isFrameAccepted", "accepted"),
    ("isFrameDropped", "dropped"),
    ("isFrameBuffered", "buffered"),
    ("isFrameOnTransit", "in_transit"),
    ("isFrameCollidedOnLink", "collided"),
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EventsRequest {
    /// Only these protocols, for example `["ICMP", "ARP"]`. Omit for every protocol.
    #[serde(default)]
    pub protocols: Vec<String>,
    /// Only events at this device.
    #[serde(default)]
    pub device: Option<String>,
    /// Include Packet Tracer's explanation of each step (the OSI decisions in PDU Details).
    #[serde(default)]
    pub include_decisions: bool,
    /// Maximum events to return, newest last. Defaults to 50, maximum 500.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SimulationEvent {
    pub index: i64,
    /// Simulation time in milliseconds.
    pub time: i64,
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// `ICMP`, `ARP`, `STP`, `DHCP`, ...
    pub protocol: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub destination: String,
    /// `accepted`, `dropped`, `buffered`, `in_transit`, `collided`.
    pub status: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EventList {
    pub events: Vec<SimulationEvent>,
    /// Events matching the filters, before `limit`.
    pub matching: usize,
    /// Every event recorded so far.
    pub total: usize,
}

pub(crate) fn simulation() -> Call {
    Call::root("simulation")
}

pub async fn list_events<P: PacketTracer>(
    packet_tracer: &P,
    request: &EventsRequest,
) -> Result<EventList, PtError> {
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let count = packet_tracer
        .call(simulation().method("getFrameInstanceCount", []))
        .await?;
    let total = usize::try_from(expect_integer(&count, "event count")?).unwrap_or_default();
    let wanted: Vec<String> = request
        .protocols
        .iter()
        .map(|protocol| protocol.trim().to_uppercase())
        .collect();

    let summaries = try_join_all((0..total).map(|index| summary(packet_tracer, index))).await?;
    let matching: Vec<SimulationEvent> = summaries
        .into_iter()
        .filter(|event| wanted.is_empty() || wanted.contains(&event.protocol))
        .filter(|event| {
            request
                .device
                .as_deref()
                .is_none_or(|device| event.device.eq_ignore_ascii_case(device.trim()))
        })
        .collect();
    let matching_count = matching.len();
    let mut events: Vec<SimulationEvent> = matching
        .into_iter()
        .skip(matching_count.saturating_sub(limit))
        .collect();
    if request.include_decisions {
        for event in &mut events {
            event.decisions = decisions(packet_tracer, event.index).await?;
        }
    }
    Ok(EventList {
        events,
        matching: matching_count,
        total,
    })
}

fn frame(index: i64) -> Call {
    simulation().method(
        "getFrameInstanceAt",
        [Value::Int(i32::try_from(index).unwrap_or(i32::MAX))],
    )
}

async fn summary<P: PacketTracer>(
    packet_tracer: &P,
    index: usize,
) -> Result<SimulationEvent, PtError> {
    let index = i64::try_from(index).unwrap_or(i64::MAX);
    let call = frame(index);
    let get = |method: &str| packet_tracer.call(call.clone().method(method, []));
    let (time, device, kind, source, destination) = tokio::try_join!(
        get("getTime"),
        packet_tracer.call(call.clone().method("getDevice", []).method("getName", [])),
        get("getUserTrafficType"),
        get("getSourceString"),
        get("getDestinationString"),
    )?;
    let from = packet_tracer
        .call(
            call.clone()
                .method("getPreviousDevice", [])
                .method("getName", []),
        )
        .await
        .ok()
        .and_then(|name| expect_text(&name, "previous device").ok());
    let flags = try_join_all(FLAGS.iter().map(|(method, _)| get(method))).await?;
    let mut status = Vec::new();
    for ((_, label), flag) in FLAGS.iter().zip(flags) {
        if expect_bool(&flag, label)? {
            status.push((*label).to_owned());
        }
    }
    Ok(SimulationEvent {
        index,
        time: expect_integer(&time, "event time")?,
        device: expect_text(&device, "event device")?,
        from,
        protocol: protocol(expect_integer(&kind, "traffic type")?),
        source: expect_text(&source, "event source")?,
        destination: expect_text(&destination, "event destination")?,
        status,
        decisions: Vec::new(),
    })
}

async fn decisions<P: PacketTracer>(packet_tracer: &P, index: i64) -> Result<Vec<String>, PtError> {
    let count = packet_tracer
        .call(frame(index).method("getFlowChartNodeCount", []))
        .await?;
    let count = expect_integer(&count, "decision count")?;
    let replies = try_join_all((0..count).map(|step| {
        packet_tracer.call(frame(index).method(
            "getDecisionAt",
            [Value::Int(i32::try_from(step).unwrap_or(i32::MAX))],
        ))
    }))
    .await?;
    replies
        .iter()
        .map(|reply| expect_text(reply, "decision"))
        .filter(|text| text.as_ref().map_or(true, |text| !text.trim().is_empty()))
        .collect()
}

pub(crate) fn protocol(code: i64) -> String {
    ApiIndex::get()
        .enum_values(TRAFFIC_ENUM)
        .and_then(|values| {
            values
                .iter()
                .find(|(_, value)| **value == code)
                .map(|(name, _)| name.trim_start_matches(TRAFFIC_PREFIX).to_owned())
        })
        .unwrap_or_else(|| format!("TYPE_{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_use_short_names() {
        assert_eq!(protocol(0), "ICMP");
        assert_eq!(protocol(11), "STP");
        assert_eq!(protocol(18), "HTTPS");
        assert_eq!(protocol(4242), "TYPE_4242");
    }
}
