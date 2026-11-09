// Written by bark-screen/scripts/qr.py generate; do not edit.

/// What the code says.
pub const ADDRESS: &str = "zond.github.io/bark";
/// The symbol's version, the smallest that holds the address at level H.
pub const VERSION: u8 = 3;
/// The data mask chosen by the standard's penalty rules.
pub const MASK: u8 = 2;
/// Modules a side: 17 + 4 × version (ISO/IEC 18004, symbol versions).
pub const SIZE: usize = 29;
/// The modules, a row each from the top, the leftmost in bit 0, 1 dark.
pub const MODULES: [u32; SIZE] = [
    0x1fc1cd7f, 0x1057e341, 0x17459b5d, 0x1746b45d, 0x175ed65d, 0x10539f41, 0x1fd5557f, 0x00177100,
    0x1ce1bd5c, 0x1ff48fa2, 0x060df75e, 0x195e5d84, 0x1c1ae3f8, 0x1ffe2f12, 0x06611150, 0x02c3cf18,
    0x1cbeea6b, 0x17a7f2b3, 0x07f2b9f5, 0x1303953d, 0x0df19651, 0x1f114b00, 0x055a447f, 0x1b1a9a41,
    0x15f4cb5d, 0x0006235d, 0x0d4d8b5d, 0x0b421a41, 0x05fa9a7f,
];
