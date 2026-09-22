# desktop

The apps on the Desktop tab of PCs, laptops and servers, driven through the
processes behind them. Routers and switches have no Desktop and are refused
with a pointer to their IOS tools.

## Tools

| Tool | App | What it does |
|---|---|---|
| `browse_web` | Web Browser | Opens a URL and returns the page, its status and the server address. |
| `configure_email` | Email | Sets name, address, user, password and the POP3 and SMTP servers. |
| `send_email` | Email | Sends a mail from that account and waits for the SMTP answer. |
| `receive_email` | Email | Presses Receive and returns the downloaded mail. |
| `vpn_client` | VPN | Connects to an Easy VPN server, disconnects, or reports the tunnel. |
| `host_files` | Text Editor | Lists, reads, writes or deletes the text files in C:\\. |

IP Configuration, Firewall and IPv6 Firewall are in
[hosts](../hosts/README.md), Command Prompt in
[host_console](../host_console/README.md) and PC Wireless in
[wireless](../wireless/README.md).

### `browse_web`

```json
{ "device": "PC1", "url": "www.gamc.bo" }
```

```json
{ "device": "PC1", "url": "http://www.gamc.bo", "status": "ok", "server": "192.168.10.10", "html": "<h1>GAMC</h1>", "text": "GAMC" }
```

`status` is one of `ok`, `not_found`, `timeout`, `host_not_found`,
`dns_server_not_found`, `unauthorized`, `connection_reset`,
`connection_closed`, `protocol_error`, `login_failed` or `invalid`, from the
`HTTPType` the browser reports. Names go through the host's own DNS server,
so a `host_not_found` usually means a missing A record or a wrong DNS server
on the host. `https://` switches the browser to HTTPS first.

### Email

```json
{ "device": "PC1", "name": "Ana", "email": "ana@gamc.bo", "username": "ana", "password": "cisco", "incoming_server": "192.168.10.10", "outgoing_server": "192.168.10.10" }
```

```json
{ "device": "PC1", "to": "luis@gamc.bo", "subject": "Informe", "body": "Adjunto el informe" }
```

- `send_email` waits for the SMTP client's `mailSent` event and fails with
  its reason (timeout, server not found, ...). A recipient the server does not
  know is still accepted, as on a real server; the failure comes back as a
  *Delivery Status Notification* on the next `receive_email`.
- The first mail after cabling can time out while ARP resolves; sending it
  again works (measured on 9.0.1).
- `receive_email` collects `mailReceived` events until two seconds pass
  without another one, or `timeout_secs` (default 10) when the mailbox is
  empty. POP3 removes the mail from the server, and Packet Tracer does not
  expose the app's inbox over IPC, so the reply is the only copy an agent
  gets.

### `vpn_client`

```json
{ "device": "PC1", "action": "connect", "server": "192.168.10.2", "group": "VPNGROUP", "group_key": "vpnkey", "username": "vpnuser", "password": "vpnpass" }
```

```json
{ "device": "PC1", "connected": true, "server": "192.168.10.2", "group": "VPNGROUP", "username": "vpnuser", "tunnel_ip": "10.50.0.11" }
```

The server is an Easy VPN server: `aaa new-model`, `crypto isakmp client
configuration group`, a dynamic crypto map and `crypto map` on the interface.
Verified on Packet Tracer 9.0.1 with a 2811, whose IOS has crypto; the 2911
needs the `securityk9` license, which did not activate in our tests, and the
ISR4331 ships with it. A wrong group or password leaves the tunnel down, and
the tool says so after `timeout_secs` (default 15).

### `host_files`

```json
{ "device": "PC1", "action": "write", "name": "notas.txt", "text": "VLAN 10: ventas" }
```

`write` creates the file or replaces its text; every answer lists the files
in C:\\ with their sizes, the same ones `dir` shows in the Command Prompt.

## What cannot be driven

Every Desktop app of Packet Tracer 9.0.1, checked against its IPC API and
tried live:

| App | Status | Why, and what to use instead |
|---|---|---|
| IP Configuration | tools | `configure_host`, `configure_host_ipv6` |
| Command Prompt | tool | `run_host_command` |
| Web Browser | tool | `browse_web` |
| PC Wireless | tool | `connect_wireless`, `wireless_status` |
| VPN | tool | `vpn_client` |
| Email | tools | `configure_email`, `send_email`, `receive_email` |
| Text Editor | tool | `host_files` |
| Firewall, IPv6 Firewall | on and off | `set_host_firewall`. The rule list has no IPC call. |
| Terminal | equivalent | The app's console session only opens from its window, but `run_cli` types at the same router or switch console directly. |
| Traffic Generator | equivalent | No IPC call. `add_pdu` sends simple PDUs and `run_host_command` sends pings with a size and count. |
| PPPoE Dialer | not available | `PPPoEClient.connect` and `connectFromPc` exist but send nothing: no PPPoE frame leaves the PC in simulation mode. |
| MIB Browser | not available | The `SnmpManager` process has no IPC methods. `run_cli` with `show snmp` checks the agent side. |
| Cisco IP Communicator | not available | No IPC class. |
| Dial-up | not available | No IPC class for the modem utility. |
| IOx IDE, Supervisory Workstation | not available | No IPC class. |

## IPC calls

| Purpose | Call |
|---|---|
| browser | `network().getDevice(d).getProcess("HttpClient")` then `setHttps(bool)`, `go(url)`; event `HttpClient.onDone(url, ip, HTTPType, html)` |
| email account | `...getProcess("EmailClient").getEmailUser()` then `set/getName`, `MailId`, `User`, `Password`, `Pop3Server`, `SmtpServer` |
| send | `...getProcess("EmailClient").getSmtpClient().sendMail(from, to, subject, body, password, server)`; event `SmtpClient.mailSent(to, subject, body, SmtpResponseType)` |
| receive | `...getProcess("EmailClient").getPop3Client().getMailIpc()`; events `Pop3Client.mailReceived(from, subject, date, body)`, `errorReceivingMail(Pop3ResponseType)` |
| VPN | `...getProcess("EasyVpnClient")` then `setServerIp`, `setGroupName`, `setGroupKey`, `setUsername`, `setPassword`, `connect()`, `disconnect()`, `isConnected()`, `getTunnelIp()` |
| files | `...getProcess("FileManager").getDirectory("c:/", false)` then `getFileCount`, `getFileAt(i)`, `fileExist`, `getFile(name).getContent(false)`, `setTextContent`, `addTextFile`, `removeFile` |
