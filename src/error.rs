use std::fmt;

/// Custom error type for hyperV operations
#[derive(Debug)]
pub enum HyperVError {
    /// I/O operation failed
    Io(std::io::Error),
    /// JSON serialization/deserialization failed
    Json(serde_json::Error),
    /// Task not found
    TaskNotFound(String),
    /// Configuration error
    Config(String),
    
    /// Task already exists
    TaskAlreadyExists(String),
    /// Task exists
    TaskExists(String),
    /// Task already running
    TaskAlreadyRunning(String),
    /// Task not running
    TaskNotRunning(String),
    /// Process operation failed
    ProcessError(String),
    /// Configuration error
    ConfigError(String),
    /// Log operation failed
    LogError(String),
    /// Invalid input provided
    InvalidInput(String),
    /// Working directory not found
    WorkdirNotFound(String),
    /// Invalid environment variable format
    InvalidEnvVar(String),
    /// Invalid log type
    InvalidLogType(String),
    /// Serialization error
    Serialization(String),
    /// Process start error
    ProcessStart(String, String), // binary, error message
    /// Process stop error
    ProcessStop(String),
    /// Binary not found
    BinaryNotFound(String),
    /// Binary not executable
    BinaryNotExecutable(String),
    /// Interpreter not found
    InterpreterNotFound(String),
    /// Invalid binary
    InvalidBinary(String),
}

impl fmt::Display for HyperVError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HyperVError::Io(err) => write!(f, "I/O error: {}", err),
            HyperVError::Json(err) => write!(f, "JSON error: {}", err),
            HyperVError::TaskNotFound(name) => write!(f, "Task '{}' not found", name),
            HyperVError::TaskAlreadyExists(name) => write!(f, "Task '{}' already exists", name),
            HyperVError::Config(msg) => write!(f, "Configuration error: {}", msg),
            HyperVError::TaskExists(name) => write!(f, "Task '{}' already exists", name),
            HyperVError::TaskAlreadyRunning(name) => write!(f, "Task '{}' is already running", name),
            HyperVError::TaskNotRunning(name) => write!(f, "Task '{}' is not running", name),
            HyperVError::ProcessError(msg) => write!(f, "Process error: {}", msg),
            HyperVError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            HyperVError::LogError(msg) => write!(f, "Log error: {}", msg),
            HyperVError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            HyperVError::WorkdirNotFound(dir) => write!(f, "Working directory not found: {}", dir),
            HyperVError::InvalidEnvVar(var) => write!(f, "Invalid environment variable format: {}", var),
            HyperVError::InvalidLogType(log_type) => write!(f, "Invalid log type: {}", log_type),
            HyperVError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            HyperVError::ProcessStart(binary, msg) => write!(f, "Failed to start process '{}': {}", binary, msg),
            HyperVError::ProcessStop(msg) => write!(f, "Failed to stop process: {}", msg),
            HyperVError::BinaryNotFound(binary) => write!(f, "Binary not found: {}", binary),
            HyperVError::BinaryNotExecutable(binary) => write!(f, "Binary not executable: {}", binary),
            HyperVError::InterpreterNotFound(interpreter) => write!(f, "Interpreter not found: {}", interpreter),
            HyperVError::InvalidBinary(msg) => write!(f, "Invalid binary: {}", msg),
        }
    }
}

impl std::error::Error for HyperVError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HyperVError::Io(err) => Some(err),
            HyperVError::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for HyperVError {
    fn from(err: std::io::Error) -> Self {
        HyperVError::Io(err)
    }
}

impl From<serde_json::Error> for HyperVError {
    fn from(err: serde_json::Error) -> Self {
        HyperVError::Json(err)
    }
}

/// Result type alias for hyperV operations
pub type Result<T> = std::result::Result<T, HyperVError>;
