#![no_std]
#![no_main]
// We make clippy (rust's linter) as annoying as possible
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
extern crate panic_halt;

mod serial;

use crate::serial::UartePort;
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use microbit::hal::rtc::RtcInterrupt;
use microbit::hal::time::Hertz;
use microbit::hal::uarte::{Baudrate, Parity};
use microbit::hal::{pwm, Clocks, Rtc};
use microbit::pac::{interrupt, RTC0, TIMER1, TIMER2, UARTE0};
use microbit::{
    board::Board,
    display::nonblocking::{Display, GreyscaleImage},
    gpio, hal,
    hal::{prelude::*, time::U32Ext, uarte, Timer},
    pac,
};

static DISPLAY: Mutex<RefCell<Option<Display<TIMER1>>>> = Mutex::new(RefCell::new(None));
static LOGIC_TIMER: Mutex<RefCell<Option<Timer<TIMER2>>>> = Mutex::new(RefCell::new(None));
static DISPLAY_TIMER: Mutex<RefCell<Option<Rtc<RTC0>>>> = Mutex::new(RefCell::new(None));
static GAME_STATE: Mutex<RefCell<Option<GameState>>> = Mutex::new(RefCell::new(None));

struct Level {
    layout: [[u8; 80]; 5],
}

struct GameState {
    player_pos: [u8; 2],
    level: Level,
    serial: UartePort<UARTE0>,
}

impl Level {
    const GROUND_PIXEL: u8 = 4u8;

    pub const fn new(layout: &[[u8; 80]; 5]) -> Self {
        Self { layout: *layout }
    }

    pub const fn get_byte_at(&self, x: u8, y: u8) -> u8 {
        if x > 80 || y > 4 {
            return 0;
        }
        self.layout[4 - y as usize][x as usize]
    }

    /*
       Parses the bytes (typically from a file) into a level instance
    */
    pub fn parse_bytes(bytes: &[u8]) -> Self {
        let mut index: usize = 0;
        let mut buf = [[0u8; 80]; 5];
        // we need to map the characters to the level format
        for i in bytes
            .iter()
            .filter(|b| **b == b'#' || **b == b'.')
            .map(|b| if *b == b'#' { Self::GROUND_PIXEL } else { 0u8 })
        {
            buf[index / 80][index % 80] = i;
            index += 1;
            if index >= 400 {
                break;
            }
        }
        Self::new(&buf)
    }
    pub fn copy_into(&self, x: u8, y: u8, data: &mut [[u8; 5]; 5]) {
        if x > 80 {
            return;
        }
        let start = if x < 2 { 0 } else { (x - 2) as usize };
        // clippy is complaining but i think it's a false positive because it's suggestion doesn't work
        for y in 0..5 {
            for x in 0..5 {
                data[y][x] = self.layout[y][start + x];
            }
        }

        // Convert the player position to screen coordinates and write it to the buffer
        if y > 4 {
            // In case the player is off-screen, the program (thankfully) panics if we try to write out-of-bounds
            return;
        }

        data[4 - y as usize][x as usize - start] = 7;
    }
}
impl GameState {
    pub fn new(serial: UartePort<UARTE0>) -> Self {
        Self {
            player_pos: [0, 1],
            level: Level::parse_bytes(include_bytes!("level1")),
            serial,
        }
    }

    pub fn add_player_y(&mut self, y: i8) {
        let old = self.player_pos[1];
        self.player_pos[1] = self.player_pos[1].wrapping_add_signed(y);
        if self
            .get_level()
            .get_byte_at(self.player_pos[0], self.player_pos[1])
            != 0
        {
            self.player_pos[1] = old;
        }
    }

    pub fn add_player_x(&mut self, x: i8) {
        let old = self.player_pos[0];
        self.player_pos[0] = self.player_pos[0].wrapping_add_signed(x);
        if self
            .get_level()
            .get_byte_at(self.player_pos[0], self.player_pos[1])
            != 0
        {
            self.player_pos[0] = old;
        }
    }

    fn add_wrapped_value_u8(val: i8, target: &mut u8) {
        *target = target.wrapping_add_signed(val);
        if *target > 4 {
            *target = if *target == u8::MAX { 4 } else { 0 };
        }
    }

    pub fn make_image(&self) -> GreyscaleImage {
        let mut grid: [[u8; 5]; 5] = [
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ];
        // Copy a section of the level into the buffer
        self.get_level()
            .copy_into(self.player_pos[0], self.player_pos[1], &mut grid);
        // Write the player position to the buffer
        //grid[4 - self.player_pos[1] as usize][self.player_pos[0] as usize] = 7u8;
        // Create and return a new GreyScaleImage
        GreyscaleImage::new(&grid)
    }

    // This returns a pointer to the current level
    pub const fn get_level<'a, 'b>(&'b self) -> &'a Level
    where
        'b: 'a,
    {
        &self.level
    }
}
#[entry]
fn main() -> ! {
    if let Some(mut board) = Board::take() {
        Clocks::new(board.CLOCK)
            .enable_ext_hfosc()
            .set_lfclk_src_synth()
            .start_lfclk();

        let mut rtc0 = Rtc::new(board.RTC0, 2047).unwrap();
        rtc0.enable_event(RtcInterrupt::Tick);
        rtc0.enable_interrupt(RtcInterrupt::Tick, None);
        rtc0.enable_counter();

        let display = Display::new(board.TIMER1, board.display_pins);

        let mut timer = Timer::new(board.TIMER0);

        let mut timer2 = Timer::new(board.TIMER2);
        timer2.start(400_000_u32);
        timer2.enable_interrupt();

        let mut _serial = {
            let serial = uarte::Uarte::new(
                board.UARTE0,
                board.uart.into(),
                Parity::EXCLUDED,
                Baudrate::BAUD115200,
            );
            UartePort::new(serial)
        };

        cortex_m::interrupt::free(move |cs| {
            *DISPLAY.borrow(cs).borrow_mut() = Some(display);
            *LOGIC_TIMER.borrow(cs).borrow_mut() = Some(timer2);
            *DISPLAY_TIMER.borrow(cs).borrow_mut() = Some(rtc0);
            *GAME_STATE.borrow(cs).borrow_mut() = Some(GameState::new(_serial));
        });

        unsafe {
            board.NVIC.set_priority(pac::Interrupt::RTC0, 64);
            board.NVIC.set_priority(pac::Interrupt::TIMER1, 128);
            board.NVIC.set_priority(pac::Interrupt::TIMER2, 32);
            pac::NVIC::unmask(pac::Interrupt::RTC0);
            pac::NVIC::unmask(pac::Interrupt::TIMER1);
            pac::NVIC::unmask(pac::Interrupt::TIMER2);
        }

        // Buttons
        let button_a = board.buttons.button_a;
        let button_b = board.buttons.button_b;
        // Handle input here
        loop {
            let mut input_a = false;
            let mut input_b = false;
            while !input_a && !input_b {
                input_a = button_a.is_low().unwrap_or(false);
                input_b = button_b.is_low().unwrap_or(false);
            }

            timer.start(50000u32);

            // try to handle double-clicking
            while timer.read() != 0 && input_a != input_b {
                if input_a {
                    input_b = button_b.is_low().unwrap_or(false);
                    if input_b {
                        break;
                    }
                }
                if input_b {
                    input_a = button_a.is_low().unwrap_or(false);
                    if input_a {
                        break;
                    }
                }
            }
            cortex_m::interrupt::free(|cs| {
                if let Some(state) = GAME_STATE.borrow(cs).borrow_mut().as_mut() {
                    //state.add_player_y(-1);
                    // hopefully this works
                    {
                        if input_a
                            && input_b
                            && state.get_level().get_byte_at(
                                state.player_pos[0],
                                state.player_pos[1].wrapping_add_signed(-1),
                            ) != 0
                        {
                            state.add_player_y(2);
                            return;
                        }
                        if input_a {
                            state.add_player_x(-1);
                        }
                        if input_b {
                            state.add_player_x(1);
                        }
                    }
                }
            });
            timer.delay_ms(250u32);
        }
    }
    panic!("End")
}

#[interrupt]
fn TIMER2() {
    cortex_m::interrupt::free(|cs| {
        if let Some(state) = GAME_STATE.borrow(cs).borrow_mut().as_mut() {
            state.add_player_y(-1);
        }
        if let Some(timer) = LOGIC_TIMER.borrow(cs).borrow_mut().as_mut() {
            timer.start(500_000_u32);
        }
    });
}

#[interrupt]
fn TIMER1() {
    cortex_m::interrupt::free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            display.handle_display_event();
        }
    });
}

#[interrupt]
fn RTC0() {
    cortex_m::interrupt::free(|cs| {
        if let Some(rtc) = DISPLAY_TIMER.borrow(cs).borrow_mut().as_mut() {
            rtc.reset_event(RtcInterrupt::Tick);
        }
    });

    let img = cortex_m::interrupt::free(|cs| {
        if let Some(state) = GAME_STATE.borrow(cs).borrow().as_ref() {
            return Some(state.make_image());
        }
        None
    });

    cortex_m::interrupt::free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            // Apparently chaining let Some statements together is "unstable" and "not supported"
            // So we have to do it this way, because of course
            try_display_img(display, img);
        }
    });
}

fn try_display_img(display: &mut Display<TIMER1>, img: Option<GreyscaleImage>) {
    if let Some(grid) = img.as_ref() {
        display.show(grid);
    }
}
