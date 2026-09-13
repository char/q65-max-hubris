# Q65 Max wireless bring-up

## SPI1

The SPI task owns PA5/PA6/PA7 and chip selects PB9 (LED0), PB8 (LED1),
PA4 (radio). Transactions are mode 0, MSB first, 8 bits, 48 MHz / 16.
The local IDL follows Hubris's atomic transaction model without its board-config
and gateway dependencies. Transfers are limited to 256 bytes and use bounded
polling; input and USB can preempt the SPI task. There is no DMA or cross-request
bus lock.

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
