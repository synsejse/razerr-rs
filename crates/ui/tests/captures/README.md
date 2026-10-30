# Actual device snapshots

These files are unmodified `razerr-rs-service status` snapshots captured on
2026-10-29 while the user physically switched a DeathAdder V3 Pro between
wired (`1532:00c2`) and wireless (`1532:00c3`) transport.

- `wired.json`: initial wired baseline, with five valid DPI stages.
- `wireless_handoff.json`: the same serial after switching to wireless, with
  fresh DPI/polling/battery and an unavailable DPI-stage read. A subsequent
  real read recovered all five stages automatically.

The regression test uses this observed failure to check that a selected
device's old editor drafts do not survive a transport change. No device data
was invented, and the test does not run a daemon or D-Bus server.
