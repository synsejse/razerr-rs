# Hardware captures

`deathadder_v3_pro.xml` was captured on 2026-10-29 from the connected
Razer DeathAdder V3 Pro (Wired), using:

```sh
busctl --user --xml-interface introspect org.razer /org/razer/device/PM2405H30011232
```

It is real daemon introspection, not a simulated device. Tests use the capture
without contacting D-Bus. This device has no editable lighting zones, so lighting
capability discovery still needs manual validation on supported hardware.

`deathadder_v3_pro_wireless.xml` was captured on the same date and with the
same command after the user physically switched this mouse to its wireless
receiver. The serial stayed the same; USB identity changed from `1532:00c2`
to `1532:00c3`. The wireless interface advertises `getSupportedPollRates`,
unlike the wired interface. Actual reads returned 125/500/1000/2000/4000/8000
Hz, with 8000 Hz selected. This capture is used to verify capability discovery
across transports; it contains no fabricated daemon methods.
