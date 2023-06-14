#![no_std]
#![no_main]
// We make clippy (rust's linter) as annoying as possible
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
extern crate panic_halt;

mod game_state;
mod serial;

use crate::game_state::GameState;
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use microbit::hal::rtc::{Instance, RtcInterrupt};
use microbit::hal::{Clocks, Rtc};
use microbit::pac::{interrupt, RTC0, TIMER1, TIMER2};
use microbit::{
    board::Board,
    display::nonblocking::{Display, GreyscaleImage},
    hal::{prelude::*, Timer},
    pac,
};

static DISPLAY: Mutex<RefCell<Option<Display<TIMER1>>>> = Mutex::new(RefCell::new(None));
static LOGIC_TIMER: Mutex<RefCell<Option<Timer<TIMER2>>>> = Mutex::new(RefCell::new(None));
static DISPLAY_TIMER: Mutex<RefCell<Option<Rtc<RTC0>>>> = Mutex::new(RefCell::new(None));
static GAME_STATE: Mutex<RefCell<Option<GameState>>> = Mutex::new(RefCell::new(None));
static WIN_SCREEN: [[u8; 5]; 5] = [
    [9u8, 9u8, 9u8, 0u8, 0u8],
    [9u8, 9u8, 9u8, 0u8, 0u8],
    [9u8, 9u8, 9u8, 0u8, 0u8],
    [0u8, 0u8, 6u8, 0u8, 0u8],
    [0u8, 0u8, 6u8, 0u8, 0u8],
];
const GRAVITY_TIMER: u32 = 300_000_u32;

#[entry]
fn main() -> ! {
    if let Some(mut board) = Board::take() {
        Clocks::new(board.CLOCK)
            .enable_ext_hfosc()
            .set_lfclk_src_synth()
            .start_lfclk();

        let rtc0 = init_rtc(board.RTC0, 2047);

        let display = Display::new(board.TIMER1, board.display_pins);

        let mut timer = Timer::new(board.TIMER0);

        let mut timer2 = Timer::new(board.TIMER2);
        timer2.start(GRAVITY_TIMER);
        timer2.enable_interrupt();

        // let serial = {
        //     let serial = uarte::Uarte::new(
        //         board.UARTE0,
        //         board.uart.into(),
        //         Parity::EXCLUDED,
        //         Baudrate::BAUD115200,
        //     );
        //     UartePort::new(serial)
        // };

        // Move these values into the static mutexes, so we can access them from interrupts
        set_mutex(display, timer2, rtc0);

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
                // check the left and right buttons
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

            /*
               Handle input logic
               We do this in the critical section so that we can borrow the game state
            */
            cortex_m::interrupt::free(|cs| {
                if let Some(state) = GAME_STATE.borrow(cs).borrow_mut().as_mut() {
                    // Don't process game logic if the player has won
                    if state.has_won() {
                        return;
                    }
                    // if both buttons are pressed and the player is on the ground, jump
                    if input_a
                        && input_b
                        && state.get_level().get_byte_at(
                            state.player_pos[0],
                            state.player_pos[1].wrapping_add_signed(-1),
                        ) != 0
                    {
                        state.add_player_y(2);
                        // This makes the gravity timer skip
                        state.jump_cancel = -2;
                        // exits the block
                        return;
                    }
                    if input_a {
                        state.add_player_x(-1);
                    }
                    if input_b {
                        state.add_player_x(1);
                    }
                }
            });
            timer.delay_ms(250u32);
        }
    }
    panic!("End")
}

fn init_rtc<T: Instance>(rtc: T, prescaler: u32) -> Rtc<T> {
    let mut rtc0 = Rtc::new(rtc, prescaler).unwrap();
    rtc0.enable_event(RtcInterrupt::Tick);
    rtc0.enable_interrupt(RtcInterrupt::Tick, None);
    rtc0.enable_counter();
    rtc0
}

fn set_mutex(display: Display<TIMER1>, timer2: Timer<TIMER2>, rtc0: Rtc<RTC0>) {
    cortex_m::interrupt::free(move |cs| {
        *DISPLAY.borrow(cs).borrow_mut() = Some(display);
        *LOGIC_TIMER.borrow(cs).borrow_mut() = Some(timer2);
        *DISPLAY_TIMER.borrow(cs).borrow_mut() = Some(rtc0);
        *GAME_STATE.borrow(cs).borrow_mut() = Some(GameState::new());
    });
}

#[interrupt]
fn TIMER2() {
    // this is like really cursed multi-threading
    cortex_m::interrupt::free(|cs| {
        if let Some(state) = GAME_STATE.borrow(cs).borrow_mut().as_mut() {
            // Don't process game logic if the player has won
            if state.has_won() {
                return;
            }

            if state.jump_cancel == 0 {
                state.add_player_y(-1);
            } else {
                state.jump_cancel += 1;
            }
        }
        if let Some(timer) = LOGIC_TIMER.borrow(cs).borrow_mut().as_mut() {
            timer.start(GRAVITY_TIMER);
        }
    });
}

/*
   Handles display events
*/
#[interrupt]
fn TIMER1() {
    cortex_m::interrupt::free(|cs| {
        if let Some(display) = DISPLAY.borrow(cs).borrow_mut().as_mut() {
            display.handle_display_event();
        }
    });
}

/*
   Handles rendering to the 5x5 pixel grid
*/
#[interrupt]
fn RTC0() {
    let img = cortex_m::interrupt::free(|cs| {
        if let Some(state) = GAME_STATE.borrow(cs).borrow().as_ref() {
            return Some(if state.has_won() {
                GreyscaleImage::new(&WIN_SCREEN)
            } else {
                state.make_image()
            });
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

    cortex_m::interrupt::free(|cs| {
        if let Some(rtc) = DISPLAY_TIMER.borrow(cs).borrow_mut().as_mut() {
            rtc.reset_event(RtcInterrupt::Tick);
        }
    });
}

fn try_display_img(display: &mut Display<TIMER1>, img: Option<GreyscaleImage>) {
    if let Some(grid) = img.as_ref() {
        display.show(grid);
    }
}
