use crate::auth::{AuthTokenBytes, AuthTokenError};
use clap::Parser;
use derive_more::Deref;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
pub struct ReflectorOpts {
    /// Modality auth token hex string used to authenticate with.
    /// Can also be provide via the MODALITY_AUTH_TOKEN environment variable.
    #[clap(
        long,
        name = "auth-token-hex-string",
        env = "MODALITY_AUTH_TOKEN",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
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
        default_value = "modality-ingest://127.0.0.1:14188",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub protocol_parent_url: Url,

    /// Allow insecure TLS
    #[clap(
        short = 'k',
        long = "insecure",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub allow_insecure_tls: bool,

    /// Use the provided UUID as the run ID instead of generating a random one
    #[clap(long, name = "run-uuid", help_heading = "REFLECTOR CONFIGURATION")]
    pub run_id: Option<Uuid>,

    /// Use the provided UUID as the time domain ID instead of generating a random one
    #[clap(
        long,
        name = "time-domain-uuid",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub time_domain: Option<Uuid>,
}

#[derive(Parser, Debug, Clone)]
pub struct TraceRecorderOpts {
    /// Instead of 'USER_EVENT @ <task-name>', use the user event channel
    /// as the event name (<channel> @ <task-name>)
    #[clap(
        long,
        name = "user-event-channel",
        conflicts_with = "user-event-format-string",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_channel: bool,

    /// Instead of 'USER_EVENT @ <task-name>', use the user event format string
    /// as the event name (<format-string> @ <task-name>)
    #[clap(
        long,
        name = "user-event-format-string",
        conflicts_with = "user-event-channel",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_format_string: bool,

    /// Use a custom event name whenever a user event with a matching
    /// channel is processed.
    /// Can be supplied multiple times.
    ///
    /// Format is '<input-channel>:<output-event-name>'.
    #[clap(
        long,
        name = "input-channel>:<output-event-name",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_channel_name: Vec<RenameMapItem>,

    /// Use a custom event name whenever a user event with a matching
    /// formatted string is processed.
    /// Can be supplied multiple times.
    ///
    /// Format is '<input-formatted-string>:<output-event-name>'.
    #[clap(
        long,
        name = "input-formatted-string>:<output-event-name",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_format_string_name: Vec<RenameMapItem>,

    /// Use custom attribute keys instead of the default 'argN' keys
    /// for user events matching the given channel and format string.
    /// Can be supplied multiple times.
    ///
    /// Format is '<channel>:<format-string>:<attr-key>[,<attr-key>]'.
    #[clap(
        long,
        name = "channel>:<format-string>:<attr-key>[,<attr-key>]",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_fmt_arg_attr_keys: Vec<FormatArgAttributeKeysItem>,

    /// Use a single timeline for all tasks instead of a timeline per task.
    /// ISRs can still be represented with their own timelines or not
    #[clap(long, help_heading = "TRACE RECORDER CONFIGURATION")]
    pub single_task_timeline: bool,

    /// Represent ISR in the parent task context timeline rather than a dedicated ISR timeline
    #[clap(long, help_heading = "TRACE RECORDER CONFIGURATION")]
    pub flatten_isr_timelines: bool,

    /// Use the provided initial startup task name instead of the default ('(startup)')
    #[clap(
        long,
        name = "startup-task-name",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub startup_task_name: Option<String>,
}

impl ReflectorOpts {
    pub(crate) fn resolve_auth(&self) -> Result<AuthTokenBytes, AuthTokenError> {
        AuthTokenBytes::resolve(self.auth_token.as_deref())
    }
}

/// A map of trace recorder USER_EVENT channels to Modality event names
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref)]
pub struct RenameMap(pub BTreeMap<String, String>);

impl FromIterator<RenameMapItem> for RenameMap {
    fn from_iter<T: IntoIterator<Item = RenameMapItem>>(iter: T) -> Self {
        Self(iter.into_iter().map(|i| (i.0, i.1)).collect())
    }
}

/// A trace recorder USER_EVENT channel to Modality event name
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct RenameMapItem(String, String);

impl FromStr for RenameMapItem {
    type Err = String;

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

/// A set of trace recorder USER_EVENT channel and format string match pairs and
/// Modality event attribute keys
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref)]
pub struct FormatArgAttributeKeysSet(pub BTreeSet<FormatArgAttributeKeysItem>);

impl FormatArgAttributeKeysSet {
    pub(crate) fn arg_attr_keys(&self, channel: &str, format_string: &str) -> Option<&[String]> {
        self.0.iter().find_map(|f| {
            if f.channel == channel && f.format_string == format_string {
                Some(f.arg_attr_keys.as_slice())
            } else {
                None
            }
        })
    }
}

impl FromIterator<FormatArgAttributeKeysItem> for FormatArgAttributeKeysSet {
    fn from_iter<T: IntoIterator<Item = FormatArgAttributeKeysItem>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// A trace recorder USER_EVENT channel and format string match pair and
/// Modality event attribute keys
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct FormatArgAttributeKeysItem {
    channel: String,
    format_string: String,
    arg_attr_keys: Vec<String>,
}

impl FromStr for FormatArgAttributeKeysItem {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_msg = |input: &str| {
            format!("Invalid format string argument attribute key item '{input}', use the supported format '<channel>:<format-string>:<attr-key>[,<attr-key>]'")
        };
        let tokens: Vec<&str> = s.split(':').collect();
        if tokens.len() != 3 {
            return Err(err_msg(s));
        }
        if tokens.iter().any(|t| t.is_empty()) {
            return Err(err_msg(s));
        }

        let arg_attr_keys: Vec<&str> = tokens[2].split(',').map(|s| s.trim()).collect();
        if arg_attr_keys.iter().any(|t| t.is_empty()) {
            return Err(err_msg(s));
        }

        Ok(Self {
            channel: tokens[0].to_owned(),
            format_string: tokens[1].to_owned(),
            arg_attr_keys: arg_attr_keys.into_iter().map(|s| s.to_owned()).collect(),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn rename_map() {
        assert_eq!(
            RenameMapItem::from_str("channel-foo:MY_FOO_EVENT"),
            Ok(RenameMapItem(
                "channel-foo".to_owned(),
                "MY_FOO_EVENT".to_owned()
            ))
        );
        assert!(RenameMapItem::from_str(":MY_FOO_EVENT").is_err());
        assert!(RenameMapItem::from_str("channel-foo:").is_err());
        assert!(RenameMapItem::from_str(":").is_err());
    }

    #[test]
    fn fmt_arg_attr_keys_set() {
        assert_eq!(
            FormatArgAttributeKeysItem::from_str("foo-ch:%u %d %bu:my_arg0,bar,baz"),
            Ok(FormatArgAttributeKeysItem {
                channel: "foo-ch".to_owned(),
                format_string: "%u %d %bu".to_owned(),
                arg_attr_keys: vec!["my_arg0".to_owned(), "bar".to_owned(), "baz".to_owned()],
            })
        );
    }
}
