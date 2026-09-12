# q65-max-hubris

rust firmware for the Keychron Q65 Max running on Hubris ^-^

i decided to do this because:

- i'm a little dissatisfied with qmk:
  - has a bunch of extraneous stuff inside it
  - it is kinda laggy if you have the scan rate too high i think
  - for a long time qmk did one keypress per usb report and like why would you do that???? so. we can do better
- hubris is coveted transgender ideologist technology
- the q65 max runs an stm32f4 so likeeee there's already support in hubris basically almost
- keyboard goes really fast now. i can totally tell the difference it's not placebo it's not i'm not coping

## non-goals

- hid over bluetooth. are you crazy????? have you seen those PDFs?? no way man
- anybody else's keymap except MINE. evil smiling imp emoji

## setup

- u may want to use [vscode-hubris](https://git.t4t.associates/char-slop/vscode-hubris) with this repo

you can build with `cargo xtask` and the output will be at `target/q65-max/dist/default/final.bin`. you can flash with dfu-util or whatever. i know you're not here to flash your keyboard; you're here for the spectacle of it. just make sure to pass `-a 0 -s 0x08000000:leave` when u do flash
