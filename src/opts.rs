use crate::auth::{AuthTokenBytes, AuthTokenError};
use clap::Parser;
use url::Url;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
pub struct CommonOpts {
    /// Modality auth token hex string used to authenticate with.
    /// Can also be provide via the MODALITY_AUTH_TOKEN environment variable.
    #[clap(long, env = "MODALITY_AUTH_TOKEN")]
    pub auth_token: Option<String>,

    /// The modalityd or modality-reflector ingest protocol parent service address
    ///
    /// The default value uses the default reflector port 14188.
    ///
    /// You can talk directly to the default ingest server port with
    /// `--ingest-protocol-parent-url modality-ingest://127.0.0.1:14182`
    #[clap(
        long = "ingest-protocol-parent-url",
        default_value = "modality-ingest://127.0.0.1:14188"
    )]
    pub protocol_parent_url: Url,

    /// Allow insecure TLS
    #[clap(short = 'k', long = "insecure")]
    pub allow_insecure_tls: bool,

    /// Use the provided UUID as the run ID instead of generating a random one
    #[clap(long)]
    pub run_id: Option<Uuid>,

    /// Use the provided UUID as the time domain ID instead of generating a random one
    #[clap(long)]
    pub time_domain: Option<Uuid>,
}

impl CommonOpts {
    pub(crate) fn resolve_auth(&self) -> Result<AuthTokenBytes, AuthTokenError> {
        AuthTokenBytes::resolve(self.auth_token.as_deref())
    }
}
