use schemars::JsonSchema;
use serde::Deserialize;

const SWITCHING_KINDS: &[&str] = &[
    "switch",
    "multi_layer_switch",
    "switch3650",
    "hub",
    "bridge",
    "repeater",
    "access_point",
    "cisco_access_point",
    "light_weight_access_point",
    "wireless_router",
    "wireless_router_new_generation",
    "home_gateway",
    "cloud",
    "dsl_modem",
    "cable_modem",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cable {
    #[default]
    Auto,
    Straight,
    Cross,
    Rollover,
    Fiber,
    FiberMultimode,
    Serial,
    Console,
    Phone,
    Coaxial,
    Octal,
    Usb,
}

impl Cable {
    pub(super) fn code(self) -> Option<i32> {
        Some(match self {
            Self::Auto => return None,
            Self::Straight => 8100,
            Self::Cross => 8101,
            Self::Rollover => 8102,
            Self::Fiber => 8103,
            Self::Phone => 8104,
            Self::Serial => 8106,
            Self::Console => 8108,
            Self::Coaxial => 8110,
            Self::Octal => 8111,
            Self::Usb => 8113,
            Self::FiberMultimode => 8117,
        })
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Straight => "straight",
            Self::Cross => "cross",
            Self::Rollover => "rollover",
            Self::Fiber => "fiber",
            Self::FiberMultimode => "fiber_multimode",
            Self::Serial => "serial",
            Self::Console => "console",
            Self::Phone => "phone",
            Self::Coaxial => "coaxial",
            Self::Octal => "octal",
            Self::Usb => "usb",
        }
    }

    pub(super) fn resolve(self, a: (&str, &str), b: (&str, &str)) -> Self {
        if self != Self::Auto {
            return self;
        }
        let (kind_a, port_a) = a;
        let (kind_b, port_b) = b;
        if is_serial(port_a) || is_serial(port_b) {
            return Self::Serial;
        }
        if is_console(port_a) || is_console(port_b) {
            return Self::Rollover;
        }
        if is_switching(kind_a) == is_switching(kind_b) {
            Self::Cross
        } else {
            Self::Straight
        }
    }
}

fn is_serial(port: &str) -> bool {
    port.starts_with("Serial")
}

fn is_console(port: &str) -> bool {
    port.eq_ignore_ascii_case("console") || port.starts_with("RS232")
}

fn is_switching(kind: &str) -> bool {
    SWITCHING_KINDS.contains(&kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(a: (&str, &str), b: (&str, &str)) -> Cable {
        Cable::Auto.resolve(a, b)
    }

    #[test]
    fn follows_the_ccna_straight_versus_cross_rule() {
        let pc = ("pc", "FastEthernet0");
        let router = ("router", "GigabitEthernet0/0");
        let switch = ("switch", "FastEthernet0/1");
        let multilayer = ("multi_layer_switch", "GigabitEthernet0/1");

        assert_eq!(auto(pc, switch), Cable::Straight);
        assert_eq!(auto(router, switch), Cable::Straight);
        assert_eq!(auto(router, multilayer), Cable::Straight);
        assert_eq!(auto(switch, multilayer), Cable::Cross);
        assert_eq!(auto(router, router), Cable::Cross);
        assert_eq!(auto(pc, router), Cable::Cross);
        assert_eq!(auto(pc, pc), Cable::Cross);
    }

    #[test]
    fn serial_and_console_ports_pick_their_own_cables() {
        assert_eq!(
            auto(("router", "Serial0/0/0"), ("router", "Serial0/0/1")),
            Cable::Serial
        );
        assert_eq!(
            auto(("pc", "RS 232"), ("router", "Console")),
            Cable::Rollover
        );
    }

    #[test]
    fn explicit_choices_are_respected() {
        let choice = Cable::Fiber.resolve(("switch", "Gig0/1"), ("switch", "Gig0/1"));
        assert_eq!(choice, Cable::Fiber);
        assert_eq!(Cable::Auto.code(), None);
        assert_eq!(Cable::FiberMultimode.code(), Some(8117));
    }
}
