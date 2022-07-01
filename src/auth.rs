use derive_more::{Deref, Into};
use thiserror::Error;
use tracing::debug;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Deref, Into)]
pub struct AuthTokenBytes(Vec<u8>);

#[derive(Copy, Clone, Debug, Error)]
pub enum AuthTokenError {
    #[error("An auth token is required. Provide one at the command line or via the MODALITY_AUTH_TOKEN environment variable.")]
    AuthRequired,

    #[error("Encountered an error decoding the auth token. {0}")]
    Hex(#[from] hex::FromHexError),
}

impl AuthTokenBytes {
    pub fn resolve(maybe_provided_hex: Option<&str>) -> Result<Self, AuthTokenError> {
        let hex = if let Some(provided_hex) = maybe_provided_hex {
            provided_hex.to_string()
        } else {
            dirs::config_dir()
                .and_then(|config| {
                    let file_path = config.join("modality_cli").join(".user_auth_token");
                    debug!("Resolving auth token from default Modality auth token file");
                    std::fs::read_to_string(file_path).ok()
                })
                .ok_or(AuthTokenError::AuthRequired)?
        };

        Ok(Self(hex::decode(hex.trim())?))
    }
}
