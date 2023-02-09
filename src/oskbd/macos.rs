//! Contains the input/output code for keyboards on macOS.

use rdev::{simulate, EventType};
use std::io;

use crate::custom_action::*;
use crate::keys::*;

/// Handle for writing keys to the OS.
pub struct KbdOut {}

impl KbdOut {
    pub fn new() -> Result<Self, io::Error> {
        Ok(Self {})
    }

    pub fn write(&mut self, event_type: EventType) -> Result<(), io::Error> {
        simulate(&event_type)
            .map_err(|_err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "We could not send keyboard event",
                )
            })
    }

    pub fn write_key(&mut self, key: OsCode, value: KeyValue) -> Result<(), io::Error> {
        let event = event_type_from_oscode(key, value);
        self.write(event)
    }

    pub fn write_code(&mut self, _code: u32, _value: KeyValue) -> Result<(), io::Error> {
        todo!();
    }

    pub fn move_mouse(
        &mut self,
        _direction: MoveDirection,
        _distance: u16,
    ) -> Result<(), io::Error> {
        todo!();
    }
    pub fn press_key(&mut self, key: OsCode) -> Result<(), io::Error> {
        self.write_key(key, KeyValue::Press)
    }

    pub fn release_key(&mut self, key: OsCode) -> Result<(), io::Error> {
        self.write_key(key, KeyValue::Release)
    }

    pub fn send_unicode(&mut self, _c: char) -> Result<(), io::Error> {
        Ok(())
    }

    pub fn click_btn(&mut self, btn: Btn) -> Result<(), io::Error> {
        log::debug!("click btn: {:?}", btn);

        Ok(())
    }

    pub fn release_btn(&mut self, btn: Btn) -> Result<(), io::Error> {
        log::debug!("release btn: {:?}", btn);

        Ok(())
    }

    pub fn scroll(&mut self, direction: MWheelDirection, distance: u16) -> Result<(), io::Error> {
        log::debug!("scroll: {direction:?} {distance:?}");
        match direction {
            MWheelDirection::Up => self.write(EventType::Wheel {
                delta_x: distance.try_into().unwrap(),
                delta_y: 0,
            }),
            MWheelDirection::Right => self.write(EventType::Wheel {
                delta_x: 0,
                delta_y: distance.try_into().unwrap(),
            }),
            MWheelDirection::Down => self.write(EventType::Wheel {
                delta_x: -(i64::try_from(distance).unwrap()),
                delta_y: 0,
            }),
            MWheelDirection::Left => self.write(EventType::Wheel {
                delta_x: 0,
                delta_y: -(i64::try_from(distance).unwrap()),
            }),
        }
    }
}

fn event_type_from_oscode(code: OsCode, value: KeyValue) -> EventType {
    match value {
        KeyValue::Release => EventType::KeyRelease(OsCode::as_key(code)),
        KeyValue::Press | KeyValue::Repeat => EventType::KeyPress(OsCode::as_key(code)),
    }
}
