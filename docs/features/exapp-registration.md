# ExApp registration

Packet Tracer only accepts PTMP connections from registered external
applications (ExApps). Registration is a one-time step per Packet Tracer
installation and survives restarts.

## 1. Pick an id and a secret

- **App id**: any reverse-domain name, for example `dev.pktctl`.
- **Secret**: a random string. It becomes `PKTCTL_SECRET`.

```bash
openssl rand -hex 24
```

## 2. Write the app meta file

Copy [pktctl-exapp.xml](pktctl-exapp.xml) and set `ID` and `KEY`.

| Privilege | Needed for |
|---|---|
| `GET_NETWORK_INFO` | `status`, `list_devices`, reading any state |
| `CHANGE_NETWORK_INFO` | `run_cli` and any tool that changes the network |
| `SIMULATION_MODE` | simulation tools |
| `CHANGE_GUI`, `MISC_GUI` | workspace, canvas and file tools |
| `CHANGE_PREFERENCES` | options such as auto cabling |
| `ACTIVITY_WIZARD`, `MULTIUSER`, `APPLICATION` | activity files, multi-user, ExApp messaging |

Granting all of them now avoids re-registering when new tools arrive. Remove
the ones you do not want pktctl to have.

`EXECUTABLE_PATH` is required by the format but pktctl is started by your MCP
client, not by Packet Tracer; keep `LOADING` as `ON_DEMAND`.

## 3. Encrypt it with Packet Tracer's `meta` tool

Packet Tracer ships the tool that turns the XML into a `.pta` file:

```bash
META="/Applications/Cisco Packet Tracer 9.0.1/Cisco Packet Tracer 9.0.1.app/Contents/extensions/meta"
"$META" pktctl.pta pktctl-exapp.xml
```

On Windows and Linux the tool lives in the `extensions` folder of the Packet
Tracer installation (not yet verified by the pktctl test suite).

## 4. Register it

In Packet Tracer: **Extensions → IPC → Configure Apps → Add**, pick
`pktctl.pta`, then **Ok**. The app appears in the list as "pktctl".

## 5. Configure pktctl

| Variable | Value |
|---|---|
| `PKTCTL_APP_ID` | the `ID` from the XML |
| `PKTCTL_SECRET` | the `KEY` from the XML |
| `PKTCTL_ADDR` | optional, defaults to `127.0.0.1:39000` |
| `PKTCTL_CALL_TIMEOUT_SECS` | optional, defaults to 30 |

Call the `status` tool: `connected: true` means everything works.

## Troubleshooting

| `status.problem` | Cause |
|---|---|
| `Packet Tracer is not reachable` | Packet Tracer is closed, or IPC listens on another port (**Extensions → IPC → Options**). |
| `rejected app id ...` | The ExApp is not registered, or `PKTCTL_SECRET` differs from the `KEY`. |

## Security

- The secret is the only thing standing between the IPC port and full control
  of Packet Tracer. Keep it out of version control.
- Packet Tracer listens on every interface. Keep **Allow Remote Applications**
  disabled in **Extensions → IPC → Options** unless you need it.
