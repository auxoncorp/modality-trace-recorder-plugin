use crate::auth::{AuthTokenBytes, AuthTokenError};
use clap::Parser;
use derive_more::Deref;
use std::collections::BTreeMap;
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
pub struct CommonOpts {
    /// Modality auth token hex string used to authenticate with.
    /// Can also be provide via the MODALITY_AUTH_TOKEN environment variable.
    #[clap(long, name = "auth-token-hex-string", env = "MODALITY_AUTH_TOKEN")]
    pub auth_token: Option<String>,

    /// The modalityd or modality-reflector ingest protocol parent service address
    ///
    /// The default value uses the default reflector port 14188.
    ///
    /// You can talk directly to the default ingest server port with
    /// `--ingest-protocol-parent-url modality-ingest://127.0.0.1:14182`
    #[clap(
        long = "ingest-protocol-parent-url",
        name = "URL",
        default_value = "modality-ingest://127.0.0.1:14188"
    )]
    pub protocol_parent_url: Url,

    /// Allow insecure TLS
    #[clap(short = 'k', long = "insecure")]
    pub allow_insecure_tls: bool,

    /// Use the provided UUID as the run ID instead of generating a random one
    #[clap(long, name = "run-uuid")]
    pub run_id: Option<Uuid>,

    /// Use the provided UUID as the time domain ID instead of generating a random one
    #[clap(long, name = "time-domain-uuid")]
    pub time_domain: Option<Uuid>,
}

impl CommonOpts {
    pub(crate) fn resolve_auth(&self) -> Result<AuthTokenBytes, AuthTokenError> {
        AuthTokenBytes::resolve(self.auth_token.as_deref())
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref)]
pub struct RenameMap(pub BTreeMap<String, String>);

impl FromIterator<RenameMapItem> for RenameMap {
    fn from_iter<T: IntoIterator<Item = RenameMapItem>>(iter: T) -> Self {
        Self(iter.into_iter().map(|i| (i.0, i.1)).collect())
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct RenameMapItem(String, String);

impl FromStr for RenameMapItem {
    type Err = String;

    // <in-name>:<out-name>
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_msg = |input: &str| {
            format!("Invalid rename map item '{input}', use the supported format '<input-name>:<output-name>'")
        };
        let tokens: Vec<&str> = s.split(':').collect();
        if tokens.len() != 2 {
            return Err(err_msg(s));
        }
        if tokens.iter().any(|t| t.is_empty()) {
            return Err(err_msg(s));
        }
        Ok(Self(tokens[0].to_string(), tokens[1].to_string()))
    }
}
