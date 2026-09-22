# hosts

IPv4 configuration of end devices: PCs, laptops, servers and other hosts.
Routers and switches are configured through IOS instead.

## Tool

`configure_host`

| Argument | Required | Meaning |
|---|---|---|
| `device` | yes | Host name. |
| `port` | no | Port to configure. Default `FastEthernet0`. |
| `dhcp` | no | `true` to request an address from DHCP. |
| `ip`, `mask` | unless `dhcp` | Static address and mask. |
| `gateway` | no | Default gateway, inside the host's subnet. |
| `dns` | no | DNS server. |

```json
{ "device": "PC1", "ip": "192.168.10.10", "mask": "255.255.255.0", "gateway": "192.168.10.1", "dns": "192.168.10.53" }
```

```json
{ "device": "PC1", "port": "FastEthernet0", "dhcp": false, "ip": "192.168.10.10", "mask": "255.255.255.0", "gateway": "192.168.10.1", "dns": "192.168.10.53" }
```

`ip`, `mask` and `dhcp` in the answer are read back from Packet Tracer.
Packet Tracer has no getter for the gateway or DNS server, so those two echo
what was applied; `run_host_command` with `ipconfig /all` shows them.

With `dhcp: true` the tool waits up to five seconds for a lease. No `ip` in the
answer means no DHCP server answered yet.

## Validation

Mistakes are caught before anything is sent to Packet Tracer:

- the mask must be contiguous (`255.0.255.0` is refused);
- the host address cannot be the network or broadcast address of its subnet
  (except on /31 and /32);
- the gateway must be inside the host's subnet;
- `dhcp` cannot be combined with a static `ip` or `mask`;
- routers, switches and firewalls are refused with a pointer to IOS
  configuration.

## IPC calls

| Purpose | Call |
|---|---|
| static | `network().getDevice(d).getPort(p: string)` then `setDhcpClientFlag(false)`, `setIpSubnetMask(ip, mask)`, `setDefaultGateway(ip)`, `setDnsServerIp(ip)` |
| DHCP | `...getPort(p).setDhcpClientFlag(true)` then `network().getDevice(d).setDhcpFlag(true)` |
| read back | `...getPort(p)` then `isDhcpClientOn()`, `getIpAddress()`, `getSubnetMask()` |

The port flag is the one that decides DHCP versus static. Verified on Packet
Tracer 9.0.1: `setDhcpFlag(false)` on the device leaves the port in DHCP mode,
while `setDhcpClientFlag(false)` on the port returns it to static cleanly.
`setDhcpFlag(true)` on the device is what starts the DHCP request.
