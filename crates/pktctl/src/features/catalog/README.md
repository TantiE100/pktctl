# catalog

Reads the hardware catalog of the running Packet Tracer, so models are never
hardcoded and always match the installed version.

## Tool

`list_models`

| Argument | Required | Meaning |
|---|---|---|
| `kind` | no | Only devices of this kind: `router`, `switch`, `multi_layer_switch`, `pc`, `server`, `laptop`, `access_point`, `asa`, `cloud`, ... |
| `include_modules` | no | Also list slot modules. Default `false`. |

```json
{ "kind": "router" }
```

```json
{ "devices": [ { "model": "1941", "kind": "router" }, { "model": "2911", "kind": "router" }, { "model": "ISR4331", "kind": "router" } ] }
```

Kinds come from `DeviceType` and `ModuleType` in the official framework (see
`packet_tracer/kinds.rs`). A model whose type is newer than the framework is
shown as `type_<code>`; Packet Tracer 9.0.1 has one, `HMI-PT` (`type_56`).

## Used by other features

- `find_device_model` resolves the model name given to `add_device` and yields
  the numeric device type Packet Tracer needs.
- `find_module_model` does the same for `add_module`.

Both accept exact names, then case-insensitive matches, and otherwise suggest
similar models.

## IPC calls

| Call | Reply |
|---|---|
| `hardwareFactory().devices().getAvailableDeviceCount()` | int `n` |
| `hardwareFactory().devices().getAvailableDeviceAt(i).getModel()` | QString |
| `hardwareFactory().devices().getAvailableDeviceAt(i).getType()` | int (`DeviceType` value) |
| same three on `modules()` with `Module` | same |

The catalog lists some models twice; duplicates are removed.
