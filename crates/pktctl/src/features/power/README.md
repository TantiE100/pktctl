# power

Device power and Realtime mode's time controls.

## Tools

| Tool | What it does |
|---|---|
| `set_power` | `{ "device": "R1", "on": false }` switches a device off or on and reads the state back. |
| `fast_forward` | Realtime toolbar's *Fast Forward Time*: timers jump ahead. |
| `power_cycle_all` | Power cycles every device that is on, the way *Power Cycle Devices* does, and returns their names. |

## Things worth knowing

- **`fast_forward` removes the waits.** A switch port that just came up spends
  about 30 seconds in spanning-tree listening and learning; DHCP, OSPF and
  EIGRP also run on timers. After `fast_forward` a ping across a new switch
  answers at once (measured: 1.1 s from cabling to replies, instead of up to
  30 s). Call it after building or reconfiguring, before checking
  connectivity.
- **Power cycling reloads IOS.** Anything not saved with `write memory` (or
  `configure_ios` with `save: true`) is lost, exactly as on real hardware.
  When `set_power` switches an IOS device on it skips the boot animation and
  answers the initial configuration dialog, so the console is ready.
- **`power_cycle_all` never presses the button.** `RealtimeToolbar.resetNetwork()`
  opens a *Reset the network?* confirmation dialog, and while it is open Packet
  Tracer answers no IPC call at all (measured on 9.0.1: every call, from every
  client, timed out until the dialog was answered). pktctl switches each device
  off and back on with `setPower` instead, which shows no dialog.
- Installing modules already handles its own power cycle; see
  [modules](../modules/README.md).

## IPC calls

| Call | Use |
|---|---|
| `network().getDevice(name: QString).getPower()`, `setPower(bool)`, `skipBoot()` | Device power. |
| `appWindow().getRealtimeToolbar().fastForwardTime()` | Fast forward. |
| `appWindow().getRealtimeToolbar().resetNetwork()` | Not used: opens a modal dialog that blocks IPC. |
