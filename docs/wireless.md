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
fails in `lib/keyboard/src/lib.rs`; its debounce implementation and assertions
are unchanged.

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
PB1 is polled at 1 ms while busy, or 8 ms when connected and quiet on battery.
Disabled/low-battery service ticks remain 100 ms.
EXTI and coordinated idle sleep remain future work.

PA4 wake/disconnect pulses are timed by the SPI server. During the low pulse no
client may clock SPI; requests return false and RGB sleeps briefly before retry.
The SPI timer deasserts PA4 even if wireless stops running. The radio's subsequent
300 ms wake delay does not monopolise the bus.

The service starts disabled and input enables it according to the physical switch.
No hardware connection or RF performance is yet verified.

## Transport routing and delivery

PA9/PA10 select wired (11), 2.4 GHz (10), or Bluetooth (01). Bluetooth is
unsupported and sends no reports; 00 is ignored as a switch transition. Startup
sends nothing until a valid selection has been stable for 100 ms.

Input owns routing. RGB queries input for the selected host's connection and LED
state rather than assuming USB. The existing keymap is unchanged. Pairing is
available through `Wireless::pair`; a physical shortcut still needs choosing.
Normal connection and module reset never issue the factory-reset/pairing-clear command.

Wireless uses the module's 20-byte NKRO format and three consumer usage slots.
A 64-snapshot queue preserves quick taps, including the encoder's immediate
press/release pair. Full queues increment the exposed overflow counter, discard
the backlog, and send all-up followed by current state. Offline taps are discarded;
reconnect sends all-up then currently held keys, not stale queued typing.

Leaving wireless drains keyboard and consumer releases before disconnecting, with
a 100 ms escape deadline if the module stops acknowledging. USB releases are
submitted to the existing USB service before changing routes.

Validation: firmware builds and 54 host tests pass across HID, USB, keyboard and
radio/power, excluding the pre-existing debounce failure noted above. Hardware checks remain:
rapid taps, encoder rotation, >6 held keys, held-key transport changes, pairing,
replugging the dongle, and RGB running concurrently. Capture ACKs to verify the
sequence-byte interpretation and measure effective report cadence/latency.

## Battery and lighting policy

PB0 senses USB power and PB13 charging (both active low, pulled up). The module
is queried for divider voltage every three seconds while connecting/connected.
The board's 560k/499k divider and QMK's approximate 3300/3500/4100 mV percentage
curve are used. `Wireless::status` exposes voltage, percentage, USB/charging,
low/critical flags, and a validity flag; voltage is stale after ten seconds.

On battery, lights stay off without a recent valid measurement. Nine seconds
below 3500 mV disables lighting; 3600 mV clears that condition. Sustained voltage
below 3300 mV for sixty seconds releases keys and disconnects the radio. This
condition latches until USB power returns. These thresholds need validation
against measured battery voltage under LED load, and are not battery protection.

Wireless lighting also turns off after ten minutes without input. PB7 then puts
both LED drivers into hardware shutdown. Wake restores their registers after a
100 ms settling delay; transport changes force reinitialisation to handle the
LED supply brown-out noted by QMK. Matrix scanning continues throughout.
Entering the ROM bootloader gives the radio 250 ms to release/disconnect first.

### Adaptive polling

With USB power absent, input scans every 8 ms after thirty seconds of inactivity.
Any sampled raw key activity or encoder edge restores 1 ms scanning immediately.
Held switches, unresolved tap-holds/buffered key events, and partial encoder detents
keep scanning fast. USB power keeps the scan interval at 1 ms, even if the host
is suspended. These are delays after a scan, not guaranteed end-to-end periods.

Connected wireless slows to 8 ms whenever its report queue and in-flight/connection
ACK work are empty on battery. Report submission, enable, and pairing requests
rearm its timer immediately. Pending reports, retries, an asserted PB1, and control
sequences retain 1 ms polling; wake/reset/disconnect deadlines are not slowed.

RGB checks at 100 ms when dark on battery, otherwise retaining its 16 ms frame
period. Resuming the lights can therefore take up to another 100 ms to notice
activity, plus the existing LED-driver settling delay; this does not delay typing.

The first idle input has roughly up to 8 ms detection latency plus scan/scheduling
time. A tap or complete encoder detent wholly between polls can be missed.
Hardware checks: wait thirty seconds on battery, exercise short taps and slow/fast
initial knob movement, hold Fn across the idle threshold, reconnect USB while idle,
and compare current draw with the backlight off. Power savings are not yet measured.

### Not implemented: coordinated MCU STOP sleep

Use USB power for initial testing. The STM32 still scans periodically and runs its
48 MHz clock; adaptive polling and low-battery radio disconnect do **not** shut down
the MCU.
Do not treat this as finished unattended battery operation or a battery-life claim.

STOP needs kernel/BSP support, not just a driver toggling SLEEPDEEP:

1. Quiesce input, USB, RGB, wireless and SPI; ensure no report, chip select, or
   lease is in flight. Resolve pending tap-holds and refuse sleep with held keys.
2. Arm row wake interrupts with columns driven low, plus radio, mode switch,
   encoder and USB-power wake sources. Check pending levels before sleeping.
3. Establish an always-on elapsed-time source (PC14/PC15 are matrix columns, so
   an LSE crystal cannot simply be assumed), and integrate elapsed sleep into
   the kernel's timer bookkeeping. SysTick alone stops in STOP.
4. Enter/leave STOP through privileged kernel/BSP code, restore the clock tree
   before any task runs, restore GPIO/peripherals, and retain the waking key's
   press/release through radio reconnect rather than dropping it as offline input.
5. Measure active/idle/sleep current and stress cold boot, USB insertion/removal,
   timer deadlines, held keys, repeated wake, and fault recovery on hardware.

No deep-sleep register writes or kernel dependency changes have been made.
