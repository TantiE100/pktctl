# preferences

Packet Tracer's Preferences window.

## Tools

| Tool | What it does |
|---|---|
| `get_preferences` | Every preference below with its current value. |
| `set_preferences` | Changes preferences by name and returns them all. |

```json
{ "values": { "show_port_labels": true, "disable_auto_cabling": true } }
```

| Name | Packet Tracer option |
|---|---|
| `show_port_labels`, `hide_port_labels_on_hover` | Always show port labels / show them only on hover |
| `show_link_lights` | Link lights |
| `hide_device_names`, `hide_device_models`, `hide_qos_stamps` | Hide tab |
| `disable_auto_cabling`, `cable_length_effects` | Cabling |
| `animation`, `sound`, `telephony_sound`, `logging`, `metric_units` | Interface |
| `external_network_access` | Allow device scripts to reach external networks |
| `cli_tab_by_default`, `show_device_taskbar`, `cable_info_popup` | Device dialogs and physical workspace |
| `hide_physical_tab`, `hide_config_tab`, `hide_cli_tab`, `hide_desktop_tab`, `hide_gui_tab` | Hide device dialog tabs, for example to set up an exam |
| `challenge_pdu_info` | Challenge mode in PDU information |
| `accessibility`, `dock_first` | Accessibility and docking |
| `show_main_toolbar`, `show_secondary_toolbar`, `show_bottom_toolbar` | Toolbars |

Preferences apply to Packet Tracer, not to the network file, and they persist
across sessions like changes made in the Preferences window.

## IPC calls

`options()` getters and setters. Their names are irregular
(`isAutoCablingDisabled` / `setDisableAutoCabling`, `isPortShown` /
`setIsPortShown`), so the feature keeps an explicit table, and a unit test
checks every entry against the official API index. The three `Hide` setters
take a second argument, `isWorkspaceActive`, sent as `true` so the change
applies to the open network at once.
