# ExApp registration

Packet Tracer only accepts PTMP connections from registered external
applications (ExApps). Registration is a one-time step per Packet Tracer
installation and survives restarts, as long as Packet Tracer quits normally after registering.

## The short way: `setup_exapp`

1. Configure pktctl with an app id and a random secret (step 1 below) and add
   it to your MCP client.
2. Ask the agent to call `setup_exapp`. pktctl renders the template below with
   your id and secret, runs Packet Tracer's own `meta` tool on it, deletes the
   plain XML and writes `~/.config/pktctl/pktctl.pta` (or `PKTCTL_SETUP_DIR`).
3. In Packet Tracer: **Extensions → IPC → Configure Apps → Add**, pick that
   `.pta`, then **Ok**.
4. `status` now reports `connected: true`.

`setup_exapp` finds `meta` in the usual install folders; set `PKTCTL_PT_HOME`
to the Packet Tracer folder if it lives elsewhere.

The manual steps below do the same by hand.

### Make it stick

Packet Tracer keeps the app list in memory and writes it to `PT.conf` only
when it **quits normally** (Cmd+Q, File > Exit). If it crashes or is forced to
quit in the same session, the registration is lost and `status` reports the
app id as rejected again. After registering, quit Packet Tracer normally once.

### Why there is no silent registration

Verified on Packet Tracer 9.0.1:

- The IPC API has no call to register an app (`IPCManager` only launches and
  messages them).
- Packet Tracer does not scan `~/Cisco Packet Tracer 9.0.1/extensions` or any
  other user folder for `.pta` files at startup.
- Opening a `.pta` with Packet Tracer (Open With, `open -a`) treats it as a
  network file and shows *Workspace is not empty*; it registers nothing.
- The Configure Apps dialog is accessible to UI automation, but driving it
  moves the user's keyboard focus and, with a list that refreshes late, can
  remove the wrong app. pktctl therefore leaves the one click to the user.
- `PT.conf` is encrypted with a key other than the `.pkt` one; editing it is
  not attempted.

## 1. Pick an id and a secret

- **App id**: any reverse-domain name, for example `dev.pktctl`.
- **Secret**: a random string. It becomes `PKTCTL_SECRET`.

```bash
openssl rand -hex 24
```

## 2. Write the app meta file

Copy [pktctl-exapp.xml](pktctl-exapp.xml) and set `ID` and `KEY`.

Packet Tracer knows eleven privileges (`SecurityPrivilege` in the framework).
The template grants all of them:

| Privilege | Needed for |
|---|---|
| `GET_NETWORK_INFO` | `status`, every `list_*` tool, reading any state |
| `CHANGE_NETWORK_INFO` | tools that change devices, links, modules, addressing or IOS |
| `CHANGE_GUI`, `MISC_GUI` | notes and screenshots on the canvas |
| `FILE` | `save_network`, `open_network`, `new_network` |
| `SIMULATION_MODE` | simulation tools |
| `CHANGE_PREFERENCES` | options such as auto cabling |
| `ACTIVITY_WIZARD`, `MULTIUSER`, `IPC`, `APPLICATION` | activity files, multi-user, ExApp messaging |

A call outside the granted privileges fails with `does not have the necessary
privilege`, which pktctl turns into an instruction to register again. Privileges
are fixed at registration time, so granting everything now avoids registering
again when new tools arrive.

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
| `PKTCTL_PT_HOME` | optional, Packet Tracer install folder for `setup_exapp` |
| `PKTCTL_SETUP_DIR` | optional, where `setup_exapp` writes the `.pta` (default `~/.config/pktctl`) |

Call the `status` tool: `connected: true` means everything works.

## Troubleshooting

| `status.problem` | Cause |
|---|---|
| `Packet Tracer is not reachable` | Packet Tracer is closed, or IPC listens on another port (**Extensions → IPC → Options**). |
| `rejected app id ...` | The ExApp is not registered, or `PKTCTL_SECRET` differs from the `KEY`. Run `setup_exapp` and register the file it creates. If it worked before a restart, Packet Tracer did not quit normally after registering; register again and quit it normally once. |
| `does not have the necessary privilege` | The ExApp was registered with fewer privileges; register the current template again. |

## Security

- The secret is the only thing standing between the IPC port and full control
  of Packet Tracer. Keep it out of version control.
- Packet Tracer listens on every interface. Keep **Allow Remote Applications**
  disabled in **Extensions → IPC → Options** unless you need it.
