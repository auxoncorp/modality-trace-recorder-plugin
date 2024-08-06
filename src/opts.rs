use clap::Parser;
use derive_more::Deref;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;
use trace_recorder_parser::types::ObjectClass;
use url::Url;

#[derive(Parser, Debug, Clone, Default)]
pub struct ReflectorOpts {
    /// Use configuration from file
    #[clap(
        long = "config",
        name = "config file",
        env = "MODALITY_REFLECTOR_CONFIG",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub config_file: Option<PathBuf>,

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
    /// The default value is `modality-ingest://127.0.0.1:14188`.
    ///
    /// You can talk directly to the default ingest server port with
    /// `--ingest-protocol-parent-url modality-ingest://127.0.0.1:14182`
    #[clap(
        long = "ingest-protocol-parent-url",
        name = "URL",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub protocol_parent_url: Option<Url>,

    /// Allow insecure TLS
    #[clap(
        short = 'k',
        long = "insecure",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub allow_insecure_tls: bool,

    /// Use the provided run ID instead of generating a random UUID
    #[clap(long, name = "run-id", help_heading = "REFLECTOR CONFIGURATION")]
    pub run_id: Option<String>,

    /// Use the provided time domain ID instead of generating a random UUID
    #[clap(
        long,
        name = "time-domain-id",
        help_heading = "REFLECTOR CONFIGURATION"
    )]
    pub time_domain: Option<String>,
}

#[derive(Parser, Debug, Clone, Default)]
pub struct TraceRecorderOpts {
    /// Instead of `USER_EVENT @ <task-name>`, use the user event channel
    /// as the event name (`<channel> @ <task-name>`)
    #[clap(
        long,
        name = "user-event-channel",
        conflicts_with = "user-event-format-string",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_channel: bool,

    /// Instead of `USER_EVENT @ <task-name>`, use the user event format string
    /// as the event name (`<format-string> @ <task-name>`)
    #[clap(
        long,
        name = "user-event-format-string",
        conflicts_with = "user-event-channel",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_format_string: bool,

    /// Instead of `USER_EVENT @ <task-name>`, use the user event format string
    /// as the event name (`<format-string> @ <task-name>`) for the given channel.
    /// Can be supplied multiple times.
    #[clap(
        long,
        name = "user-event-format-string-channel",
        conflicts_with = "user-event-format-string",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_format_string_channel: Vec<String>,

    /// Use a custom event name whenever a user event with a matching
    /// channel is processed.
    /// Can be supplied multiple times.
    ///
    /// Format is `<input-channel>:<output-event-name>`.
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
    /// Format is `<input-formatted-string>:<output-event-name>`.
    #[clap(
        long,
        name = "input-formatted-string>:<output-event-name",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub user_event_formatted_string_name: Vec<RenameMapItem>,

    /// Use custom attribute keys instead of the default 'argN' keys
    /// for user events matching the given channel and format string.
    /// Can be supplied multiple times.
    ///
    /// Format is `<channel>:<format-string>:<attr-key>[,<attr-key>]`.
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

    /// Don't synthesize interactions between tasks and ISRs when a context switch occurs
    #[clap(long, help_heading = "TRACE RECORDER CONFIGURATION")]
    pub disable_task_interactions: bool,

    /// Detect task/ISR timeline IDs from the device by reading events on the 'modality_timeline_id'
    /// channel.
    ///
    /// Format is `name=<obj-name>,id=<timeline-id>`
    #[clap(long, help_heading = "TRACE RECORDER CONFIGURATION")]
    pub use_timeline_id_channel: bool,

    /// Parse Deviant custom events using the provided base event ID.
    ///
    /// When enabled, Deviant related information will be parsed and mapped to their
    /// reserved Modality event names and attributes.
    ///
    /// Event data:
    /// * Event: `modality.mutator.announced`
    ///   - Event ID offset: 0
    ///   - data: `['mutator_id']`
    ///     - `mutator_id` is a 16-byte UUID array
    /// * Event: `modality.mutator.retired`
    ///   - Event ID offset: 1
    ///   - data: `['mutator_id']`
    ///     - `mutator_id` is a 16-byte UUID array
    /// * Event: `modality.mutation.command_communicated`
    ///   - Event ID offset: 2
    ///   - data: `['mutator_id', 'mutation_id', 'mutation_success']`
    ///     - `mutator_id` is a 16-byte UUID array
    ///     - `mutation_id` is a 16-byte UUID array
    ///     - `mutation_success` is a 4-byte (uint32_t) boolean
    /// * Event: `modality.mutation.clear_communicated`
    ///   - Event ID offset: 3
    ///   - data: `['mutator_id', 'mutation_id', 'mutation_success']`
    ///     - `mutator_id` is a 16-byte UUID array
    ///     - `mutation_id` is a 16-byte UUID array
    ///     - `mutation_success` is a 4-byte (uint32_t) boolean
    /// * Event: `modality.mutation.triggered`
    ///   - Event ID offset: 4
    ///   - data: `['mutator_id', 'mutation_id', 'mutation_success']`
    ///     - `mutator_id` is a 16-byte UUID array
    ///     - `mutation_id` is a 16-byte UUID array
    ///     - `mutation_success` is a 4-byte (uint32_t) boolean
    /// * Event: `modality.mutation.injected`
    ///   - Event ID offset: 5
    ///   - data: `['mutator_id', 'mutation_id', 'mutation_success']`
    ///     - `mutator_id` is a 16-byte UUID array
    ///     - `mutation_id` is a 16-byte UUID array
    ///     - `mutation_success` is a 4-byte (uint32_t) boolean
    #[clap(
        long,
        verbatim_doc_comment,
        value_parser=clap_num::maybe_hex::<u16>,
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub deviant_event_id_base: Option<u16>,

    /// Parse custom printf events using the provided event ID.
    #[clap(
        long,
        value_parser=clap_num::maybe_hex::<u16>,
        verbatim_doc_comment,
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub custom_printf_event_id: Option<u16>,

    /// Use the provided initial startup task name instead of the default ('(startup)')
    #[clap(
        long,
        name = "startup-task-name",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub startup_task_name: Option<String>,

    /// Specify an object class to ignore.
    /// These events will be omitted during processing.
    /// Can be supplied multiple times.
    #[clap(
        long,
        name = "ignore-object-class",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub ignore_object_class: Vec<IgnoredObjectClass>,

    /// Include unknown events instead of ignoring them.
    #[clap(
        long,
        name = "include-unknown-events",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub include_unknown_events: bool,

    /// Interaction mode to use.
    ///
    /// * fully-linearized
    /// * ipc
    #[clap(
        long,
        verbatim_doc_comment,
        name = "interaction-mode",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub interaction_mode: Option<InteractionMode>,

    /// CPU utilization measurement window duration (Default is 500ms).
    #[clap(
        long,
        name = "cpu-utilization-measurement-window",
        help_heading = "TRACE RECORDER CONFIGURATION"
    )]
    pub cpu_utilization_measurement_window: Option<humantime::Duration>,
}

/// A map of trace recorder USER_EVENT channels/format-strings to Modality event names
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref, serde::Deserialize,
)]
pub struct RenameMap(#[serde(default)] pub BTreeSet<RenameMapItem>);

impl RenameMap {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find_map(|m| {
            if m.input == key {
                Some(m.event_name.as_str())
            } else {
                None
            }
        })
    }
}

impl FromIterator<RenameMapItem> for RenameMap {
    fn from_iter<T: IntoIterator<Item = RenameMapItem>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

// TODO - refactor this to be a map, key is input
/// A trace recorder USER_EVENT channel/format-string to Modality event name
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RenameMapItem {
    #[serde(alias = "channel", alias = "formatted-string")]
    pub(crate) input: String,
    pub(crate) event_name: String,
}

impl FromStr for RenameMapItem {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err_msg = |input: &str| {
            format!("Invalid rename map item '{input}', use the supported format '<input-name>:<event-name>'")
        };
        let tokens: Vec<&str> = s.split(':').collect();
        if tokens.len() != 2 {
            return Err(err_msg(s));
        }
        if tokens.iter().any(|t| t.is_empty()) {
            return Err(err_msg(s));
        }
        Ok(Self {
            input: tokens[0].to_string(),
            event_name: tokens[1].to_string(),
        })
    }
}

// TODO - refactor this to be a map, key is (channel, format_string)
/// A set of trace recorder USER_EVENT channel and format string match pairs and
/// Modality event attribute keys
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref, serde::Deserialize,
)]
pub struct FormatArgAttributeKeysSet(#[serde(default)] pub BTreeSet<FormatArgAttributeKeysItem>);

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
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FormatArgAttributeKeysItem {
    pub(crate) channel: String,
    pub(crate) format_string: String,
    #[serde(rename = "attribute-keys")]
    pub(crate) arg_attr_keys: Vec<String>,
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

#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Deref, serde_with::DeserializeFromStr,
)]
pub struct IgnoredObjectClass(pub ObjectClass);

impl FromStr for IgnoredObjectClass {
    type Err = <ObjectClass as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(IgnoredObjectClass(ObjectClass::from_str(s)?))
    }
}

#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Deref, serde::Deserialize,
)]
pub struct IgnoredObjectClasses(#[serde(default)] pub BTreeSet<IgnoredObjectClass>);

impl FromIterator<IgnoredObjectClass> for IgnoredObjectClasses {
    fn from_iter<T: IntoIterator<Item = IgnoredObjectClass>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl FromIterator<ObjectClass> for IgnoredObjectClasses {
    fn from_iter<T: IntoIterator<Item = ObjectClass>>(iter: T) -> Self {
        Self(iter.into_iter().map(IgnoredObjectClass).collect())
    }
}

impl IgnoredObjectClasses {
    pub fn contains(&self, c: ObjectClass) -> bool {
        self.0.contains(&IgnoredObjectClass(c))
    }
}

#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, serde_with::DeserializeFromStr,
)]
pub enum InteractionMode {
    #[default]
    FullyLinearized,
    Ipc,
}

impl FromStr for InteractionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "fully-linearized" => InteractionMode::FullyLinearized,
            "ipc" => InteractionMode::Ipc,
            _ => return Err(format!("Invalid interaction mode '{}'", s)),
        })
    }
}

impl std::fmt::Display for InteractionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InteractionMode::FullyLinearized => f.write_str("fully-linearized"),
            InteractionMode::Ipc => f.write_str("ipc"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn rename_map() {
        assert_eq!(
            RenameMapItem::from_str("channel-foo:MY_FOO_EVENT"),
            Ok(RenameMapItem {
                input: "channel-foo".to_owned(),
                event_name: "MY_FOO_EVENT".to_owned()
            })
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
