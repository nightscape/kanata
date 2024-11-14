use super::*;


pub const NORMAL_KEY_ROW: u8 = 0;
pub const FAKE_KEY_ROW: u8 = 1;

pub(crate) fn get_fake_key_coords<T: Into<usize>>(y: T) -> (u8, u16) {
    let y: usize = y.into();
    (FAKE_KEY_ROW, y as u16)
}
