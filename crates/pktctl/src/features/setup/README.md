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
    "Click Ok. The registration survives restarts; call status to confirm."
  ]
}
```

## How it works

1. Renders [docs/features/pktctl-exapp.xml](../../../../../docs/features/pktctl-exapp.xml),
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
Tracer exposes no API to register an app.
