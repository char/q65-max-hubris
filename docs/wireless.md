# Q65 Max wireless bring-up

## SPI1

The SPI task owns PA5/PA6/PA7 and chip selects PB9 (LED0), PB8 (LED1),
PA4 (radio). Transactions are mode 0, MSB first, 8 bits, 48 MHz / 16.
The local IDL follows Hubris's atomic transaction model without its board-config
and gateway dependencies. Transfers are limited to 256 bytes and use bounded
polling. Hubris requires servers to outrank their callers, so priority order is
USB, SPI, wireless, input, RGB. LED writes are split into 32-register transactions
to bound scan interference (34 bytes, about 91 us on the wire, plus software
overhead). There is no DMA or cross-request bus lock.

RGB retains ownership of PB7 (LED shutdown). GPIO configuration happens during
task startup, before entering the normal request loops; runtime pin changes use
BSRR rather than read/modify/write of shared GPIO registers.

Validation: `cargo xtask`, `cargo test -p hid`, and `cargo test -p usb-device` pass.
The existing keyboard test `debounce_believes_the_first_edge_and_ignores_bounces`
fails at `lib/keyboard/src/lib.rs:93`; SPI changes do not touch that code.

Hardware checks still required:

- Wired typing and RGB after cold boot and DFU boot.
- Scope CS/SCK and confirm 3 MHz, mode 0, no overlapping chip selects.
- Measure scan latency during continuous RGB updates.
- Exercise MISO with the radio; LED writes alone cannot validate received data.

## LKBT51 protocol

`lib/lkbt51` implements command framing, Q65 module configuration, HID report
conversion, and status/ACK decoding. Its fixtures follow the Keychron QMK source,
not captures from this keyboard. `cargo test -p lkbt51` and strict host clippy pass.
Reads allocate 68 bytes: four SPI prefix bytes plus 64 module response bytes.
Incomplete/unknown event packets are discarded; radio DFU and its larger/fragmented
responses are intentionally unsupported. The module firmware is not replaced.

## Wireless task

The wireless task owns PB1 (active-low event input) and PC4 (reset). It exposes
report submission, enable/disable, pairing, and connection/LED/error status.
The timed link state machine is host-testable. ACK loss retransmits the same
sequence and packet up to three attempts, then resets/reconfigures the module.
PB1 is polled at 1 ms for bring-up; EXTI and lower idle polling rates are future
power work.

PA4 wake/disconnect pulses are timed by the SPI server. During the low pulse no
client may clock SPI; requests return false and RGB sleeps briefly before retry.
The SPI timer deasserts PA4 even if wireless stops running. The radio's subsequent
300 ms wake delay does not monopolise the bus.

This stage leaves routing wired-only; the wireless service is disabled until
its enable API is called. No hardware connection or RF performance is yet verified.
