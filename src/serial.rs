use core::fmt;
use core::fmt::Write;
use core::ops::Add;
use microbit::hal::uarte::{Instance, UarteRx, UarteTx};
use microbit::hal::Uarte;

pub struct UartePort<T: Instance>(UarteTx<T>, UarteRx<T>);

static mut TX_BUF: [u8; 1] = [0; 1];
static mut RX_BUF: [u8; 1] = [0; 1];

impl<T: Instance> UartePort<T> {
    pub fn new(serial: Uarte<T>) -> UartePort<T> {
        let (tx, rx) = serial
            .split(unsafe { &mut TX_BUF }, unsafe { &mut RX_BUF })
            .unwrap();
        UartePort(tx, rx)
    }

    pub fn write_str(&mut self, s: &str) -> fmt::Result {
        // Only write to the serial during debugging
        #[cfg(debug_assertions)]
        self.0.write_str(s)
    }
}
