use ptmp::{Step, TypeCode, Value};

use super::remote::{Remote, check_args, int_arg, no_args, string_arg};

const IPV4_PROCESS: &str = "AclProcess";
const IPV6_PROCESS: &str = "Aclv6Process";
const ACL: &str = "ACL";
const ANY_IPV4_MASK: &str = "255.255.255.255";
const HOST_IPV4_MASK: &str = "0.0.0.0";
const ANY_IPV6_PREFIX: &str = "0";
const HOST_IPV6_PREFIX: &str = "128";

/// The ACLs of a device, the ones a host's Firewall app writes into ACL 101.
#[derive(Debug, Clone, Default)]
pub(super) struct Acls {
    ipv4: Vec<(String, Vec<String>)>,
    ipv6: Vec<(String, Vec<String>)>,
}

pub(super) fn serves(name: &str) -> bool {
    matches!(name, IPV4_PROCESS | IPV6_PROCESS)
}

pub(super) fn process(acls: &mut Acls, name: &str, steps: &[Step]) -> Result<Value, Remote> {
    let ipv6 = name == IPV6_PROCESS;
    let lists = if ipv6 { &mut acls.ipv6 } else { &mut acls.ipv4 };
    match steps {
        [step] if step.method == "getClassName" => Ok(Value::string(name)),
        [step] if step.method == "addAcl" => {
            let id = string_arg(step, name)?.to_owned();
            if !lists.iter().any(|(existing, _)| *existing == id) {
                lists.push((id, Vec::new()));
            }
            Ok(Value::Void)
        }
        [step] if step.method == "getAclCount" => {
            no_args(step, name).map(|()| Value::Int(i32::try_from(lists.len()).unwrap_or(i32::MAX)))
        }
        [pick, rest @ ..] if pick.method == "getAcl" => {
            let id = string_arg(pick, name)?.to_owned();
            let statements = lists
                .iter_mut()
                .find(|(existing, _)| *existing == id)
                .map(|(_, statements)| statements)
                .ok_or_else(|| Remote::missing("Acl"))?;
            acl(statements, ipv6, rest)
        }
        [step, ..] => Err(Remote::unknown_method(name, &step.method)),
        [] => Err(Remote::unknown_method(name, "")),
    }
}

fn acl(statements: &mut Vec<String>, ipv6: bool, steps: &[Step]) -> Result<Value, Remote> {
    let [step] = steps else {
        let method = steps.first().map_or("", |step| step.method.as_str());
        return Err(Remote::unknown_method(ACL, method));
    };
    match step.method.as_str() {
        "getCommandCount" | "getStatementCount" => no_args(step, ACL)
            .map(|()| Value::Int(i32::try_from(statements.len()).unwrap_or(i32::MAX))),
        "getCommandAt" => {
            let index = int_arg(step, ACL)?;
            usize::try_from(index)
                .ok()
                .and_then(|index| statements.get(index))
                .map(Value::string)
                .ok_or_else(|| Remote::missing("ACLStatement"))
        }
        "addExtStatement" | "removeExtStatement" => {
            check_args(
                step,
                ACL,
                &[
                    TypeCode::Bool,
                    TypeCode::String,
                    TypeCode::Bool,
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::Int,
                    TypeCode::String,
                    TypeCode::String,
                    TypeCode::Int,
                ],
            )?;
            let text = |at: usize| step.args[at].as_str().unwrap_or_default().to_owned();
            let port = |at: usize| step.args[at].as_i64().unwrap_or_default();
            let statement = render(
                &text(1),
                step.args[2].as_bool().unwrap_or_default(),
                (&text(3), &text(4), port(5)),
                (&text(6), &text(7), port(8)),
                ipv6,
            );
            if step.method == "addExtStatement" {
                statements.push(statement);
                return Ok(Value::Bool(true));
            }
            let found = statements.iter().position(|kept| *kept == statement);
            if let Some(index) = found {
                statements.remove(index);
            }
            Ok(Value::Bool(found.is_some()))
        }
        other => Err(Remote::unknown_method(ACL, other)),
    }
}

fn render(
    protocol: &str,
    permit: bool,
    remote: (&str, &str, i64),
    local: (&str, &str, i64),
    ipv6: bool,
) -> String {
    let action = if permit { "permit" } else { "deny" };
    format!(
        "{action} {protocol} {} {}",
        endpoint(remote, ipv6),
        endpoint(local, ipv6)
    )
}

fn endpoint((ip, mask, port): (&str, &str, i64), ipv6: bool) -> String {
    let (any, host) = if ipv6 {
        (ANY_IPV6_PREFIX, HOST_IPV6_PREFIX)
    } else {
        (ANY_IPV4_MASK, HOST_IPV4_MASK)
    };
    let where_from = if mask == any {
        "any".to_owned()
    } else if mask == host {
        format!("host {ip}")
    } else {
        format!("{ip} {mask}")
    };
    if port == 0 {
        where_from
    } else {
        format!("{where_from} eq {port}")
    }
}
