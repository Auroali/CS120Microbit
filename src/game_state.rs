use crate::serial::UartePort;
use microbit::display::nonblocking::GreyscaleImage;
use microbit::pac::UARTE0;

pub struct Level {
    layout: [[u8; 80]; 5],
}

pub struct GameState {
    pub player_pos: [u8; 2],
    pub jump_cancel: i8,
    level: [Level; GameState::LEVEL_COUNT],
    current_level: usize,
    has_won: bool,
    serial: UartePort<UARTE0>,
}

impl Level {
    pub const GROUND_PIXEL: u8 = 4u8;
    pub const MARKER_PIXEL: u8 = 1u8;
    pub const FLAG_PIXEL: u8 = 6u8;
    // Completely blank level
    const EMPTY: Level = Level {
        layout: [[0u8; 80]; 5],
    };

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
            .filter(|b| **b == b'#' || **b == b'.' || **b == b'F' || **b == b'm')
            .map(|b| {
                if *b == b'#' {
                    Self::GROUND_PIXEL
                } else if *b == b'F' {
                    Self::FLAG_PIXEL
                } else if *b == b'm' {
                    Self::MARKER_PIXEL
                } else {
                    0u8
                }
            })
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

        data[4 - y as usize][x as usize - start] = 9;
    }
}
impl GameState {
    const LEVEL_COUNT: usize = 4;

    pub fn new(serial: UartePort<UARTE0>) -> Self {
        Self {
            player_pos: [0, 1],
            level: [
                Level::parse_bytes(include_bytes!("level1")),
                Level::parse_bytes(include_bytes!("level2")),
                Level::parse_bytes(include_bytes!("level3")),
                Level::parse_bytes(include_bytes!("level4")),
            ],
            current_level: 0,
            jump_cancel: 0,
            has_won: false,
            serial,
        }
    }

    pub fn add_player_y(&mut self, y: i8) {
        let old = self.player_pos[1];
        self.player_pos[1] = self.player_pos[1].wrapping_add_signed(y);
        if self.player_pos[1] > 15 {
            self.current_level = 0;
            self.player_pos[0] = 0;
            self.player_pos[1] = 1;
        }
        if self
            .get_level()
            .get_byte_at(self.player_pos[0], self.player_pos[1])
            == Level::GROUND_PIXEL
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
            == Level::FLAG_PIXEL
        {
            self.next_level();
            return;
        }

        if self
            .get_level()
            .get_byte_at(self.player_pos[0], self.player_pos[1])
            == Level::GROUND_PIXEL
        {
            self.player_pos[0] = old;
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

    pub fn next_level(&mut self) {
        self.player_pos[0] = 0;
        self.player_pos[1] = 1;
        if self.current_level + 1 == self.level.len() {
            self.has_won = true;
            return;
        }
        self.current_level += 1;
    }

    pub const fn has_won(&self) -> bool {
        self.has_won
    }

    // This returns a pointer to the current level
    pub const fn get_level<'a, 'b>(&'b self) -> &'a Level
    where
        'b: 'a,
    {
        &self.level[self.current_level]
    }
}
