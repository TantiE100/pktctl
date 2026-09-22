# activity

Activity files (`.pka`): the Activity Wizard's instructions, progress and
checks, plus any file's description.

## Tools

| Tool | What it does |
|---|---|
| `activity_status` | Percentage complete, score, assessment items and points, instruction pages, seconds elapsed or left, whether the password is confirmed. For a `.pkt` it only reports `is_activity: false`. |
| `activity_instructions` | One page of instructions (`page` from 1) as readable text and as HTML. |
| `check_activity` | Runs the connectivity tests and returns their lines with the current status, like *Check Results*. |
| `reset_activity` | Resets the activity to its initial network. |
| `unlock_activity` | Gives a password-protected activity its password (for its author or instructor). |
| `network_description` | Reads the file's description, or replaces it with `text`. |

```json
{
  "file": "/labs/vlans.pka", "is_activity": true,
  "percent_complete": 75.0, "score_percent": 75.0,
  "items": { "correct": 3, "total": 4 }, "points": { "correct": 3, "total": 4 },
  "instruction_pages": 2, "seconds_elapsed": 125, "seconds_left": 600,
  "password_confirmed": true
}
```

Open an activity with `open_network`, work through it with the other tools,
and read the progress here. Instructions and descriptions are rich-text HTML;
`text` strips styles, scripts and tags and keeps one line per paragraph.

## Things worth knowing

- **Password-protected activities**, such as the Cisco Networking Academy
  labs, keep their scores and checks behind the activity's password: Packet
  Tracer answers *Activity file requires password* until
  `ActivityFile.confirmPassword` succeeds. Their instructions stay readable.
  `activity_status` then reports `password_confirmed: false` and only the
  instruction pages; `check_activity` and `reset_activity` explain that
  `unlock_activity` is needed. pktctl never tries to recover a password: the
  protection is the activity's assessment.
- Verified live with the NetAcad lab *3.3.12 VLAN Configuration*
  (password-protected, from a public repository) and with a sample shipped with
  Packet Tracer (unprotected, no assessment items). The score and item counts of
  an unlocked graded activity are covered by the canvas tests; they are Packet
  Tracer's own numbers passed through.
- The live test opens a copy of a sample activity shipped with Packet Tracer
  (`saves/06 Industrial - OT/.../latching-with-plc.pka`), never the original.
  That sample has no assessment items, so its counters are zero; course
  activities report their real counts.
- Authoring (the Activity Wizard itself, answer networks, variables, scripts)
  is reachable through `call_ipc` on `ActivityFile` and `NetworkFile`.

## IPC calls

`appWindow().getActiveFile()` returns a `NetworkFile`, an `ActivityFile` for
`.pka`: `isActivityFile`, `getPercentageComplete`, `getPercentageCompleteScore`,
`get[Correct]Assessment{Items,Score}Count`, `getInstructionCount`,
`getInstruction(int)`, `getTimeElapsed`, `getTimerType`,
`getCountDownTime[Left]`, `isPasswordConfirmed`, `runConnectivityTests`,
`getConnectivityCount`, `getLastConnectivityTestCorrectCount`,
`getLastConnectivityTestResultAt(int)`, `resetActivity`,
`get/setNetworkDescription`.
