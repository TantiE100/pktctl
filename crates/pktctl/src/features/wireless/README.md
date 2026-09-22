# wireless

Access points, wireless routers and wireless clients.

## Tools

| Tool | What it does |
|---|---|
| `configure_access_point` | SSID, security, key and SSID broadcast of an access point or wireless router. |
| `connect_wireless` | Joins a client (laptop, PC, tablet with a wireless card) to a network and addresses its radio. |
| `wireless_status` | Settings of an access point, or settings and associated access point of a client. |

`security` is `open`, `wep`, `wpa_psk` or `wpa2_psk`:

| Security | Packet Tracer authentication / encryption | Key |
|---|---|---|
| `open` | Disabled / none | none |
| `wep` | WEP / WEP 64-bit or 128-bit | 10 or 26 hexadecimal digits |
| `wpa_psk` | WPA-PSK / TKIP | 8 to 63 characters |
| `wpa2_psk` | WPA2-PSK / AES | 8 to 63 characters |

```json
{ "device": "LT1", "ssid": "GAMC", "security": "wpa2_psk", "key": "clave1234",
  "ip": "192.168.50.20", "mask": "255.255.255.0", "gateway": "192.168.50.1" }
```

```json
{ "device": "LT1", "ssid": "GAMC", "associated": true, "access_point": "AP1",
  "ip": "192.168.50.20", "file": "/Users/me/labs/gamc.pkt" }
```

## How association works in Packet Tracer 9.0.1

Verified live, and why `connect_wireless` works the way it does:

- A client joins using its **current profile** (the *PC Wireless* app's
  profile), not the SSID and key in its Config tab. The IPC call that sets it,
  `WirelessClientProcess.setCurrentProfile`, fails inside Packet Tracer 9.0.1
  with an empty error.
- Clients only associate when their radio starts: when the network is opened
  or the wireless card is installed. Changing settings afterwards,
  `resetAllAssociations` (which also stops the radio from seeing any network
  until the file is reopened), power cycling or waiting do not make a client
  join again.

So `connect_wireless`:

1. Sets SSID, authentication, encryption and key in the client's wireless
   process, so its Config tab shows the same values.
2. Saves the network, rewrites the client's `CURRENT_PROFILE` in the file with
   [pktfile](../../../../pktfile/README.md), reopens it and removes the power
   units Packet Tracer adds on open, as `rename_location` does.
3. Waits up to 15 seconds, pressing Fast Forward, for `getCurrentApMac`, and
   maps that MAC to the access point whose radio port has it.
4. Addresses the radio port like `configure_host`: DHCP, or the static IP,
   mask, gateway and DNS.

A wrong key or SSID leaves `associated: false`. The network ends up saved, to
its current file or to a temporary file if it was never saved.

- **Range is physical.** Packet Tracer puts new access points in the main
  wiring closet's rack and laptops in the office, which is out of radio range.
  Move the access point next to its clients with
  [`move_to_location`](../physical/README.md) first.
- A wired laptop or PC has no radio: install one with `add_module`, for
  example `PT-LAPTOP-NM-1W` in a laptop's slot `0`. The error says so.
- `configure_access_point` changes the access point at once, but clients that
  are already associated stay associated until they reconnect.

## IPC calls

| Call | Use |
|---|---|
| `network().getDevice(name).getProcess("WirelessServerProcess" / "WirelessClientProcess")` | The radio's process. |
| `setSsid(string)`, `setAuthenType(WirelessAuthenType)`, `setEncryptType(WirelessEncryptType)` | Settings. |
| `getWpaProcess().setKey(string)`, `getWepProcess().setKey(string)` | Keys. |
| `setSsidBrdCastEnabled(bool)` | SSID broadcast (access points). |
| `getCurrentApMac()` | The access point a client is associated with, empty if none. |
| `Port.isWirelessPort()`, `Port.getMacAddress()` | Finding radios and matching the MAC. |
