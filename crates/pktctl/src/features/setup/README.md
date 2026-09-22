# setup

Creates the file that registers pktctl with Packet Tracer, so first-time setup
does not require editing XML or running tools by hand.

## Tool

`setup_exapp`, no arguments. It works before Packet Tracer accepts pktctl,
which is exactly when it is needed.

```json
{
  "app_id": "dev.pktctl",
  "pta": "/Users/me/.config/pktctl/pktctl.pta",
  "meta_tool": "/Applications/Cisco Packet Tracer 9.0.1/Cisco Packet Tracer 9.0.1.app/Contents/extensions/meta",
  "steps": [
    "Open Packet Tracer and choose Extensions > IPC > Configure Apps.",
    "Click Add and select /Users/me/.config/pktctl/pktctl.pta.",
    "Click Ok and call status to confirm. Quit Packet Tracer normally once (Cmd+Q or File > Exit) so it saves the registration."
  ]
}
```

## How it works

1. Renders [assets/pktctl-exapp.xml](../../../assets/pktctl-exapp.xml),
   compiled into the binary, with `PKTCTL_APP_ID` and `PKTCTL_SECRET`. Both are
   restricted to letters, digits, dots, dashes and underscores so they cannot
   break the XML.
2. Finds Packet Tracer's `meta` tool: `PKTCTL_PT_HOME` when set, otherwise the
   newest `Cisco Packet Tracer*` install under `/Applications`,
   `C:\Program Files` or `/opt`.
3. Writes the XML with owner-only permissions, runs `meta <pta> <xml>` and
   deletes the XML whatever the outcome, so the secret only remains inside the
   encrypted `.pta`.

The last click, **Add** in Packet Tracer's dialog, stays with the user: Packet
Tracer exposes no API to register an app and does not pick `.pta` files up by
itself (see [ExApp registration](../../../../../docs/features/exapp-registration.md#why-there-is-no-silent-registration)).
Packet Tracer saves the registration only when it quits normally, so the
steps end with quitting it once.
