# hosts

Addressing and firewall of end devices: PCs, laptops, servers and other hosts,
the IP Configuration, Firewall and IPv6 Firewall apps of their Desktop.
Routers and switches are configured through IOS instead.

## Tools

| Tool | What it does |
|---|---|
| `configure_host` | IPv4: DHCP, or static address, mask, gateway and DNS. |
| `configure_host_ipv6` | IPv6: `static` with `address/prefix`, `auto` (SLAAC) or `off`; gateway and DNS. |
| `set_host_firewall` | Switches the IPv4 and IPv6 inbound firewalls on or off, and adds or removes their rules. |

### `configure_host`

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

#### Validation

Mistakes are caught before anything is sent to Packet Tracer:

- the mask must be contiguous (`255.0.255.0` is refused);
- the host address cannot be the network or broadcast address of its subnet
  (except on /31 and /32);
- the gateway must be inside the host's subnet;
- `dhcp` cannot be combined with a static `ip` or `mask`;
- routers, switches and firewalls are refused with a pointer to IOS
  configuration.

### `configure_host_ipv6`

```json
{ "device": "PC1", "address": "2001:db8:10::20/64", "gateway": "fe80::1", "dns": "2001:db8:10::10" }
```

```json
{ "device": "PC1", "port": "FastEthernet0", "mode": "static", "addresses": ["2001:db8:10::20/64"], "gateway": "fe80::1", "dns": "2001:db8:10::10" }
```

`static` replaces the port's addresses with the one given; `auto` removes them
and waits up to five seconds for SLAAC, which needs a router sending
advertisements (`ipv6 unicast-routing`); `off` switches IPv6 off. The address
must carry its prefix and cannot be link-local, multicast or `::`. Verified on
Packet Tracer 9.0.1: `ipconfig /all` shows the address, gateway and DNS
server.

### `set_host_firewall`

```json
{ "device": "PC1", "ipv4": true, "add_rules": [
  { "action": "deny", "protocol": "icmp" },
  { "action": "permit", "protocol": "tcp", "port": 80, "remote_ip": "192.168.10.10", "remote_mask": "0.0.0.0" }
] }
```

```json
{ "device": "PC1", "port": "FastEthernet0", "ipv4": true, "ipv6": false,
  "ipv4_rules": ["deny icmp any any", "permit tcp host 192.168.10.10 any eq 80"], "ipv6_rules": [] }
```

Each of `ipv4` and `ipv6` is optional; the answer reports both switches and
every rule. Rules are evaluated in order, like an ACL: a `permit` before a
`deny` wins, which is what the Firewall app shows.

- `remote_mask` is an IPv4 wildcard (`0.0.0.0` one host, `255.255.255.255`
  any) or an IPv6 prefix length (`128` one host, `0` any).
- `port` only applies to tcp and udp. Packet Tracer keeps a single port per
  rule and matches on it, so that is what the tool sends.
- `remove_rules` must describe an existing rule exactly, and Packet Tracer
  refuses to add the same rule twice.
- Verified on 9.0.1: with the firewall on, `deny icmp any any` drops pings to
  that host.

The rules live in the ACL numbered 101 of the host's `AclProcess` (and
`Aclv6Process`), which is where the Firewall app writes them; the tool
creates that ACL the first time.

## IPC calls

| Purpose | Call |
|---|---|
| static | `network().getDevice(d).getPort(p: string)` then `setDhcpClientFlag(false)`, `setIpSubnetMask(ip, mask)`, `setDefaultGateway(ip)`, `setDnsServerIp(ip)` |
| DHCP | `...getPort(p).setDhcpClientFlag(true)` then `network().getDevice(d).setDhcpFlag(true)` |
| read back | `...getPort(p)` then `isDhcpClientOn()`, `getIpAddress()`, `getSubnetMask()` |
| IPv6 | `...getPort(p)` then `setIpv6Enabled(bool)`, `setIpv6AddressAutoConfig(bool)`, `removeAllIpv6Addresses()`, `addIpv6Address(ip, prefix, UNICAST, false)`, `setv6DefaultGateway(ip)`, `setv6ServerIp(ip)`, `getIpv6Addresses()` |
| firewall | `...getPort(p)` then `setInboundFirewallService(bool)`, `setInboundIpv6FirewallService(bool)`, `isInboundFirewallOn()`, `isInboundIpv6FirewallOn()` |
| firewall rules | `network().getDevice(d).getProcess("AclProcess"\|"Aclv6Process")` then `addAcl("101")`, `getAcl("101")` and `addExtStatement`, `removeExtStatement`, `getCommandCount`, `getCommandAt` |

The port flag is the one that decides DHCP versus static. Verified on Packet
Tracer 9.0.1: `setDhcpFlag(false)` on the device leaves the port in DHCP mode,
while `setDhcpClientFlag(false)` on the port returns it to static cleanly.
`setDhcpFlag(true)` on the device is what starts the DHCP request.
