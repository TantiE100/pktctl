# services

The Services tab of a Server-PT: DHCP, DNS, HTTP/HTTPS, FTP, email, NTP,
Syslog and TFTP.

## Tools

| Tool | What it does |
|---|---|
| `list_server_services` | Every service and whether it is on. |
| `set_server_service` | Switches one service on or off: `dhcp`, `dns`, `http`, `https`, `ftp`, `smtp`, `pop3`, `ntp`, `syslog`, `tftp`. |
| `configure_dhcp_server` | Switches DHCP on and creates or updates pools. |
| `configure_dns_server` | Switches DNS on and adds `A`, `CNAME` or `NS` records. |
| `set_web_page` | Writes a page of the HTTP service. |
| `add_server_user` | Adds an FTP account (permissions from `RWDNL`) or an email account, optionally setting the mail domain. |

```json
{
  "device": "SRV1",
  "pools": [
    { "name": "serverPool", "gateway": "192.168.10.1", "start_ip": "192.168.10.100",
      "mask": "255.255.255.0", "dns": "192.168.10.5", "max_users": 50 },
    { "name": "VLAN20", "gateway": "192.168.20.1", "start_ip": "192.168.20.100",
      "mask": "255.255.255.0" }
  ]
}
```

The reply lists every pool as Packet Tracer stores it, including the network
and the last address it computed (`192.168.10.149` for 50 users from `.100`).
The server's own pool is called `serverPool` and is updated in place; other
names create new pools. A server's DHCP service belongs to one of its ports,
`FastEthernet0` unless `port` says otherwise.

`configure_dns_server` reads the records back, so the reply shows what clients
will resolve. Adding a record that already exists fails with an explanation.

## Things worth knowing

- The DHCP service hands out leases only after the server has an address in
  the pool's network: give the server a static address with
  `configure_host` first.
- Pair these tools with `fast_forward` and `configure_host` with
  `"dhcp": true` on clients to see the leases at once.

## IPC calls

| Process (`getProcess(name)`) | Calls |
|---|---|
| `DhcpServerMainProcess` | `getDhcpServerProcessByPortName(port)` then `isEnable/setEnable`, `getPool/getPoolAt/getPoolCount`, `addNewPool(name, gateway, dns, start, mask, maxUsers, tftp, wlc)` (all strings but `maxUsers`), pool setters with IPv4 values. |
| `DnsServerProcess` | `isEnabled/setEnable`, `addARecordToNameServerDb`, `addCNAMEToNameServerDb`, `addNSRecordToNameServerDb`, `getSizeOfNameServerDb`, `getRrFromNameServerDbAt` (a `DnsRrA`/`DnsRrCname`/`DnsRrNs` value object). |
| `HttpServer`, `HttpsServer` | `isEnabled/setEnable`, `isHttpsEnabled/setHttpsEnable`, `setPageContents`, `getPage`. |
| `FtpServer` | `isEnabled/setEnabled`, `getFtpUserAccountManager().addFtpUser/isExistingUser`. |
| `EmailServer`, `SmtpServer`, `Pop3Server` | `addUser`, `setServerDomainName`, `isEnabled/setEnable`. |
| `NtpServer`, `SyslogServer`, `TftpServer` | Their enable getter and setter. |

The process names are the ones Packet Tracer 9.0.1 answers to; the Java
interfaces are named differently (`DNSServerProcess`, `HTTPServer`).
