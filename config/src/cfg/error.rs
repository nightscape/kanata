use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    pub msg: String,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Self { msg }
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err(Error::from(format!($($arg)*)))
    };
}

#[macro_export]
macro_rules! anyhow {
    ($($arg:tt)*) => {
        Error::from(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! bail_expr {
    ($expr:expr, $($arg:tt)*) => {
        bail!("Error in expression {:?}: {}", $expr, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! anyhow_expr {
    ($expr:expr, $($arg:tt)*) => {
        anyhow!("Error in expression {:?}: {}", $expr, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! bail_span {
    ($span:expr, $($arg:tt)*) => {
        bail!("Error in line {}: {}", $span.line, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! anyhow_span {
    ($span:expr, $($arg:tt)*) => {
        anyhow!("Error in line {}: {}", $span.line, format!($($arg)*))
    };
}
