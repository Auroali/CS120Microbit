# CS120Microbit
Extremely basic platformer game for the microbit, for a cs120 project.

Written in rust.

## Compilation
Follow the guide at [rust-embedded](https://docs.rust-embedded.org/discovery/microbit/03-setup/index.html) to get all the required tools.

You will also need the rust target `thumbv7em-none-eabihf` for the microbit v2

Then just run `cargo build` and flash the built binary in `target/thumbv7em-none-eabihf/debug` to the microbit.
