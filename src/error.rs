use auxon_sdk::ingest_client::{IngestClientInitializationError, IngestError};
use std::io;
use trace_recorder_parser::types::{
    KernelPortIdentity, ObjectHandle, UserEventArgRecordCount, STARTUP_TASK_NAME,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Missing startup task ('{}') object properties", STARTUP_TASK_NAME)]
    MissingStartupTaskProperties,

    #[error("Failed to locate task properties for object handle {0}")]
    TaskPropertiesLookup(ObjectHandle),

    #[error("Failed to locate ISR properties for object handle {0}")]
    IsrPropertiesLookup(ObjectHandle),

    #[error("Kernel port {0} is not supported")]
    UnsupportedKernelPortIdentity(KernelPortIdentity),

    #[error("Failed to locate a timeline for task object handle {0}")]
    TaskTimelineLookup(ObjectHandle),

    #[error("Failed to locate a timeline for ISR object handle {0}")]
    IsrTimelineLookup(ObjectHandle),

    #[error(
        "User events are only allowed to up to {} events",
        UserEventArgRecordCount::MAX
    )]
    ExceededMaxUserEventArgs,

    #[error("The user event format string '{0}' argument count doesn't match the provided custom attribute key set '{1:?}'")]
    FmtArgAttrKeysCountMismatch(String, Vec<String>),

    #[error(transparent)]
    StreamingProtocol(#[from] trace_recorder_parser::streaming::Error),

    #[error("Encountered an error with the Deviant custom event configuration. {0}.")]
    DeviantEvent(String),

    #[error("Encountered an ingest client initialization error. {0}")]
    IngestClientInitialization(#[from] IngestClientInitializationError),

    #[error("Encountered an ingest client error. {0}")]
    Ingest(#[from] IngestError),

    #[error(transparent)]
    Auth(#[from] AuthTokenError),

    #[error("Encountered an error attemping to parse the protocol parent URL. {0}")]
    Url(#[from] url::ParseError),

    #[error(
        "Encountered and IO error while reading the input stream ({})",
        .0.kind()
    )]
    Io(#[from] io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum AuthTokenError {
    #[error(transparent)]
    StringDeserialization(#[from] auxon_sdk::auth_token::AuthTokenStringDeserializationError),

    #[error(transparent)]
    LoadAuthTokenError(#[from] auxon_sdk::auth_token::LoadAuthTokenError),
}
