#[rustfmt::skip]

mod all {
    pub const PATTERN_STABLE_BLOCK: &[&str] = &[
        "XX", 
        "XX"
    ];
    pub const PATTERN_STABLE_LOAF: &[&str] = &[
        " XX ", 
        "X  X", 
        " X X", 
        "  X ", 
    ];
    pub const PATTERN_STABLE_BEEHIVE: &[&str] = &[
        " XX ", 
        "X  X", 
        " XX ", 
    ];
    pub const PATTERN_STABLE_SHIP: &[&str] = &[
        " XXX", 
        "X  X", 
        "XXX ", 
    ];
    pub const PATTERN_STABLE_BOAT: &[&str] = &[
        "XX ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_FLOWER: &[&str] = &[
        " X ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_POND: &[&str] = &[
        " X ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_LIST: &[&[&str]] = &[
        PATTERN_STABLE_BLOCK,
        PATTERN_STABLE_LOAF,
        PATTERN_STABLE_BEEHIVE,
        PATTERN_STABLE_SHIP,
        PATTERN_STABLE_BOAT,
        PATTERN_STABLE_FLOWER,
        PATTERN_STABLE_POND,
    ];

    pub const PATTERN_CLOCK_BLINKER: &[&str] = &[
        "XXX", 
    ];
    pub const PATTERN_CLOCK_TOAD: &[&str] = &[
        " XXX",
        "XXX ",
    ];
    pub const PATTERN_CLOCK_TRAFIC_LIGHT: &[&str] = &[
        "  XXX  ", 
        "       ",
        "X     X",
        "X     X",
        "X     X",
        "       ",
        "  XXX  ",
    ];
    pub const PATTERN_CLOCK_BEACON: &[&str] = &[
        "XX  ", 
        "XX  ", 
        "  XX", 
        "  XX", 
    ];
    pub const PATTERN_CLOCK_PULSAR: &[&str] = &[
        "  XXX   XXX  ", 
        "             ",
        "X    X X    X", 
        "X    X X    X", 
        "X    X X    X", 
        "  XXX   XXX  ", 
        "             ",
        "  XXX   XXX  ", 
        "X    X X    X", 
        "X    X X    X", 
        "X    X X    X", 
        "             ",
        "  XXX   XXX  ", 
    ];
    pub const PATTERN_CLOCK_I_COLUMN: &[&str] = &[
        "XXX",
        "X X",
        "XXX",
        "XXX",
        "XXX",
        "XXX",
        "X X",
        "XXX",
    ];
    pub const PATTERN_CLOCK_LIST: &[&[&str]] = &[
        PATTERN_CLOCK_BLINKER,
        PATTERN_CLOCK_TOAD,
        PATTERN_CLOCK_TRAFIC_LIGHT,
        PATTERN_CLOCK_BEACON,
        PATTERN_CLOCK_PULSAR,
        PATTERN_CLOCK_I_COLUMN,
    ];

    pub const PATTERN_FLY_GLIDER: &[&str] = &[
        " X ",
        "  X",
        "XXX",
    ];
    pub const PATTERN_FLY_LIGHTWEIGHT_SPACESHIP: &[&str] = &[
        "X  X ",
        "    X",
        "X   X",
        " XXXX",
    ];
    pub const PATTERN_FLY_LIST: &[&[&str]] = &[
        PATTERN_FLY_GLIDER,
        PATTERN_FLY_LIGHTWEIGHT_SPACESHIP,
    ];
}

pub use all::*;
