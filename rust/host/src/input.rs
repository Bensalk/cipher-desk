//! Keyboard and mouse from the viewer, passed to this computer.

use std::sync::{Arc, Mutex};

use ironrdp_server::{KeyboardEvent, MouseEvent, RdpServerInputHandler};

/// Mouse buttons the viewer can press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// One input action, in screen pixels and PC keyboard scancodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key {
        scancode: u8,
        extended: bool,
        down: bool,
    },
    Unicode {
        unit: u16,
        down: bool,
    },
    MoveTo {
        x: u16,
        y: u16,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// Positive = away from the user (scroll up), in wheel units (120 = one notch).
    Wheel {
        delta: i16,
    },
}

/// Something that performs input on this computer.
pub trait InputSink: Send {
    fn send(&mut self, input: Input);
}

/// Records input (tests).
#[derive(Clone, Default)]
pub struct RecordingSink(pub Arc<Mutex<Vec<Input>>>);

impl InputSink for RecordingSink {
    fn send(&mut self, input: Input) {
        if let Ok(mut v) = self.0.lock() {
            v.push(input);
        }
    }
}

/// Ignores input (view-only sharing).
pub struct NoInput;

impl InputSink for NoInput {
    fn send(&mut self, _: Input) {}
}

/// Turns the server's input events into [`Input`] for a sink.
pub(crate) struct Handler<S: InputSink> {
    pub sink: S,
}

impl<S: InputSink> RdpServerInputHandler for Handler<S> {
    fn keyboard(&mut self, event: KeyboardEvent) {
        let input = match event {
            KeyboardEvent::Pressed { code, extended } => Input::Key {
                scancode: code,
                extended,
                down: true,
            },
            KeyboardEvent::Released { code, extended } => Input::Key {
                scancode: code,
                extended,
                down: false,
            },
            KeyboardEvent::UnicodePressed(unit) => Input::Unicode { unit, down: true },
            KeyboardEvent::UnicodeReleased(unit) => Input::Unicode { unit, down: false },
            // Lock-key state from the viewer is not forced onto this computer.
            KeyboardEvent::Synchronize(_) => return,
        };
        self.sink.send(input);
    }

    fn mouse(&mut self, event: MouseEvent) {
        let input = match event {
            MouseEvent::Move { x, y } => Input::MoveTo { x, y },
            MouseEvent::LeftPressed => Input::Button {
                button: Button::Left,
                down: true,
            },
            MouseEvent::LeftReleased => Input::Button {
                button: Button::Left,
                down: false,
            },
            MouseEvent::RightPressed => Input::Button {
                button: Button::Right,
                down: true,
            },
            MouseEvent::RightReleased => Input::Button {
                button: Button::Right,
                down: false,
            },
            MouseEvent::MiddlePressed => Input::Button {
                button: Button::Middle,
                down: true,
            },
            MouseEvent::MiddleReleased => Input::Button {
                button: Button::Middle,
                down: false,
            },
            MouseEvent::VerticalScroll { value } => Input::Wheel { delta: value },
            // Side buttons, horizontal and relative movement are not used by the viewer.
            _ => return,
        };
        self.sink.send(input);
    }
}
