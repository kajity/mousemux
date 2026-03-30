use crate::config::ConfigError;
use crate::device::DeviceError;
use crate::virtual_keyboard::VirtualKeyboardError;
use crate::virtual_mouse::VirtualMouseError;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    Config(ConfigError),
    Device(DeviceError),
    Mouse(VirtualMouseError),
    Keyboard(VirtualKeyboardError),
    Signal(std::io::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "{err}"),
            Self::Device(err) => write!(f, "{err}"),
            Self::Mouse(err) => write!(f, "{err}"),
            Self::Keyboard(err) => write!(f, "{err}"),
            Self::Signal(err) => write!(f, "failed to install signal handler: {err}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<ConfigError> for AppError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<DeviceError> for AppError {
    fn from(value: DeviceError) -> Self {
        Self::Device(value)
    }
}

impl From<VirtualMouseError> for AppError {
    fn from(value: VirtualMouseError) -> Self {
        Self::Mouse(value)
    }
}

impl From<VirtualKeyboardError> for AppError {
    fn from(value: VirtualKeyboardError) -> Self {
        Self::Keyboard(value)
    }
}
