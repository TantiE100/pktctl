use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::packet_tracer::{
    PtError,
    api::{ApiIndex, MethodDef},
};

const DEFAULT_LIMIT: usize = 40;
const MAX_LIMIT: usize = 200;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DescribeRequest {
    /// Show one class with every method it offers, inherited ones included, for example `Router`.
    #[serde(default)]
    pub class: Option<String>,
    /// Search class names, method names and summaries, for example `ssid` or `simulation`.
    #[serde(default)]
    pub search: Option<String>,
    /// Show the values of one enum, for example `ConnectType`.
    #[serde(default, rename = "enum")]
    pub enum_name: Option<String>,
    /// Show the events an object class raises, for example `LogicalWorkspace`, for
    /// `watch_events`. An empty string lists every class that raises events.
    #[serde(default)]
    pub events: Option<String>,
    /// Maximum number of search results. Defaults to 40, maximum 200.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MethodInfo {
    pub class: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ClassInfo {
    pub name: String,
    pub ancestors: Vec<String>,
    pub subclasses: Vec<String>,
    pub methods: Vec<MethodInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EnumInfo {
    pub name: String,
    pub values: Vec<(String, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EventInfo {
    pub class: String,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Overview {
    pub roots: Vec<(String, String)>,
    pub classes: usize,
    pub methods: usize,
    pub enums: usize,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Description {
    Class(ClassInfo),
    Enum(EnumInfo),
    Events {
        event_classes: Vec<EventInfo>,
    },
    Matches {
        matches: Vec<MethodInfo>,
        truncated: bool,
    },
    Overview(Overview),
}

pub fn describe(request: &DescribeRequest) -> Result<Description, PtError> {
    let api = ApiIndex::get();
    let trimmed = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    if let Some(name) = trimmed(&request.class) {
        return class_info(api, &name);
    }
    if let Some(name) = request.events.as_deref().map(str::trim) {
        return event_info(api, name);
    }
    if let Some(name) = trimmed(&request.enum_name) {
        return enum_info(api, &name);
    }
    if let Some(query) = trimmed(&request.search) {
        let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        return Ok(search(api, &query, limit));
    }
    Ok(Description::Overview(overview(api)))
}

fn remote_methods(api: &ApiIndex) -> impl Iterator<Item = (&str, &MethodDef)> {
    api.classes
        .iter()
        .filter(|(_, class)| class.remote)
        .flat_map(|(name, class)| {
            class
                .methods
                .iter()
                .filter(|method| !method.local)
                .map(move |method| (name.as_str(), method))
        })
}

fn info(class: &str, method: &MethodDef) -> MethodInfo {
    MethodInfo {
        class: class.to_owned(),
        signature: method.signature(),
        doc: method.doc.clone(),
    }
}

fn find_class<'a>(api: &'a ApiIndex, name: &str) -> Option<&'a str> {
    api.classes
        .keys()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
        .map(String::as_str)
}

fn class_info(api: &ApiIndex, name: &str) -> Result<Description, PtError> {
    let Some(name) = find_class(api, name).filter(|name| api.classes[*name].remote) else {
        let lower = name.to_lowercase();
        let similar: Vec<&str> = api
            .classes
            .iter()
            .filter(|(candidate, class)| class.remote && candidate.to_lowercase().contains(&lower))
            .map(|(candidate, _)| candidate.as_str())
            .take(15)
            .collect();
        return Err(PtError::NotFound(format!(
            "IPC class `{name}`{}",
            if similar.is_empty() {
                String::new()
            } else {
                format!(" (similar: {})", similar.join(", "))
            }
        )));
    };
    let ancestors = api.ancestors(name);
    let methods = ancestors
        .iter()
        .flat_map(|owner| {
            api.classes[*owner]
                .methods
                .iter()
                .filter(|method| !method.local)
                .map(|method| info(owner, method))
        })
        .collect();
    Ok(Description::Class(ClassInfo {
        name: name.to_owned(),
        ancestors: ancestors[1..]
            .iter()
            .map(|owner| (*owner).to_owned())
            .collect(),
        subclasses: api
            .descendants(name)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        methods,
    }))
}

fn enum_info(api: &ApiIndex, name: &str) -> Result<Description, PtError> {
    let (name, values) = api
        .enums
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .ok_or_else(|| PtError::NotFound(format!("IPC enum `{name}`")))?;
    let mut values: Vec<(String, i64)> = values
        .iter()
        .map(|(label, value)| (label.clone(), *value))
        .collect();
    values.sort_by_key(|(_, value)| *value);
    Ok(Description::Enum(EnumInfo {
        name: name.clone(),
        values,
    }))
}

fn event_info(api: &ApiIndex, name: &str) -> Result<Description, PtError> {
    let event_classes = if name.is_empty() {
        api.events
            .iter()
            .map(|(class, events)| EventInfo {
                class: class.clone(),
                events: events.clone(),
            })
            .collect()
    } else {
        let (class, events) = api
            .event_class(name)
            .ok_or_else(|| PtError::NotFound(format!("event class `{name}`")))?;
        vec![EventInfo {
            class: class.to_owned(),
            events: events.to_vec(),
        }]
    };
    Ok(Description::Events { event_classes })
}

fn search(api: &ApiIndex, query: &str, limit: usize) -> Description {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut scored: Vec<(usize, MethodInfo)> = remote_methods(api)
        .filter_map(|(class, method)| {
            let score = words
                .iter()
                .map(|word| relevance(class, method, word))
                .try_fold(0, |total, score| score.map(|score| total + score))?;
            Some((score, info(class, method)))
        })
        .collect();
    scored.sort_by(|(left, _), (right, _)| right.cmp(left));
    let truncated = scored.len() > limit;
    let matches = scored
        .into_iter()
        .take(limit)
        .map(|(_, method)| method)
        .collect();
    Description::Matches { matches, truncated }
}

fn relevance(class: &str, method: &MethodDef, word: &str) -> Option<usize> {
    let name = method.name.to_lowercase();
    let doc = method.doc.as_deref().unwrap_or_default().to_lowercase();
    let whole_word = |text: &str| {
        text.split(|character: char| !character.is_alphanumeric())
            .any(|token| token == word)
    };
    if name.starts_with(&format!("get{word}"))
        || name.starts_with(&format!("set{word}"))
        || name.starts_with(word)
    {
        Some(4)
    } else if whole_word(&doc) || class.to_lowercase() == word {
        Some(3)
    } else if name.contains(word) || class.to_lowercase().contains(word) {
        Some(2)
    } else if doc.contains(word) {
        Some(1)
    } else {
        None
    }
}

fn overview(api: &ApiIndex) -> Overview {
    Overview {
        roots: api
            .roots
            .iter()
            .map(|(root, class)| (root.clone(), class.clone()))
            .collect(),
        classes: api.classes.values().filter(|class| class.remote).count(),
        methods: remote_methods(api).count(),
        enums: api.enums.len(),
        hint: "Every call_ipc path starts at a root or at an object uuid. Use `search` to find \
               methods by keyword, `class` to list what an object offers, and `enum` to read \
               the values an argument accepts."
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_lists_roots_and_totals() {
        let Description::Overview(overview) = describe(&DescribeRequest::default()).unwrap() else {
            panic!("expected an overview");
        };
        assert!(
            overview
                .roots
                .contains(&("simulation".into(), "Simulation".into()))
        );
        assert!(overview.methods > 3000);
    }

    #[test]
    fn class_view_includes_inherited_methods() {
        let request = DescribeRequest {
            class: Some("router".into()),
            ..DescribeRequest::default()
        };
        let Description::Class(class) = describe(&request).unwrap() else {
            panic!("expected a class");
        };
        assert_eq!(class.name, "Router");
        assert!(class.ancestors.contains(&"Device".to_owned()));
        assert!(
            class.methods.iter().any(
                |method| method.class == "Device" && method.signature.starts_with("addModule(")
            )
        );
        assert!(
            !class
                .methods
                .iter()
                .any(|method| method.signature.starts_with("getFactory"))
        );
    }

    #[test]
    fn search_matches_every_word() {
        let request = DescribeRequest {
            search: Some("physical move".into()),
            ..DescribeRequest::default()
        };
        let Description::Matches { matches, .. } = describe(&request).unwrap() else {
            panic!("expected matches");
        };
        assert!(
            matches
                .iter()
                .any(|method| method.signature.starts_with("moveIntoObject("))
        );
    }

    #[test]
    fn ranks_real_matches_above_accidental_substrings() {
        let request = DescribeRequest {
            search: Some("ssid".into()),
            ..DescribeRequest::default()
        };
        let Description::Matches { matches, .. } = describe(&request).unwrap() else {
            panic!("expected matches");
        };
        assert!(
            matches[0].signature.to_lowercase().contains("ssid"),
            "{matches:?}"
        );
        let accidental = matches
            .iter()
            .position(|method| method.signature.starts_with("getProcessId"));
        let real = matches
            .iter()
            .position(|method| method.signature.starts_with("setSsid"))
            .unwrap();
        assert!(accidental.is_none_or(|accidental| accidental > real));
    }

    #[test]
    fn enums_come_sorted_by_wire_value() {
        let request = DescribeRequest {
            enum_name: Some("connecttype".into()),
            ..DescribeRequest::default()
        };
        let Description::Enum(values) = describe(&request).unwrap() else {
            panic!("expected an enum");
        };
        assert_eq!(values.values[0], ("ETHERNET_STRAIGHT".into(), 8100));
    }

    #[test]
    fn lists_the_events_of_a_class() {
        let request = DescribeRequest {
            events: Some("logicalworkspace".into()),
            ..DescribeRequest::default()
        };
        let Description::Events { event_classes } = describe(&request).unwrap() else {
            panic!("expected events");
        };
        assert_eq!(event_classes[0].class, "LogicalWorkspace");
        assert!(event_classes[0].events.contains(&"deviceAdded".to_owned()));
    }

    #[test]
    fn unknown_classes_suggest_close_names() {
        let request = DescribeRequest {
            class: Some("Wireless".into()),
            ..DescribeRequest::default()
        };
        let error = describe(&request).unwrap_err().to_string();
        assert!(error.contains("similar"), "{error}");
    }
}
