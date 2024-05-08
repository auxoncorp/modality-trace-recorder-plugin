use crate::{
    error::AuthTokenError,
    opts::{
        FormatArgAttributeKeysSet, IgnoredObjectClasses, InteractionMode, ReflectorOpts, RenameMap,
        TraceRecorderOpts,
    },
};
use auxon_sdk::{
    auth_token::AuthToken,
    reflector_config::{Config, TomlValue, TopLevelIngest, CONFIG_ENV_VAR},
};
use derive_more::{Deref, From, Into};
use serde::Deserialize;
use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum TraceRecorderConfigEntry {
    Importer,
    TcpCollector,
    ItmCollector,
    RttCollector,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TraceRecorderConfig {
    pub auth_token: Option<String>,
    pub ingest: TopLevelIngest,
    pub plugin: PluginConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PluginConfig {
    pub run_id: Option<Uuid>,
    pub time_domain: Option<Uuid>,
    pub startup_task_name: Option<String>,
    pub single_task_timeline: bool,
    pub flatten_isr_timelines: bool,
    pub disable_task_interactions: bool,
    pub use_timeline_id_channel: bool,
    pub deviant_event_id_base: Option<u16>,
    pub include_unknown_events: bool,
    pub ignored_object_classes: IgnoredObjectClasses,
    pub user_event_channel: bool,
    pub user_event_format_string: bool,
    pub user_event_channel_rename_map: RenameMap,
    pub user_event_formatted_string_rename_map: RenameMap,
    pub user_event_fmt_arg_attr_keys: FormatArgAttributeKeysSet,
    pub interaction_mode: InteractionMode,

    pub import: ImportConfig,
    pub tcp_collector: TcpCollectorConfig,
    pub itm_collector: ItmCollectorConfig,
    pub rtt_collector: RttCollectorConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ImportConfig {
    pub protocol: Option<String>,
    pub file: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TcpCollectorConfig {
    pub disable_control_plane: bool,
    pub restart: bool,
    pub connect_timeout: Option<HumanTime>,
    pub remote: Option<SocketAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ItmCollectorConfig {
    pub disable_control_plane: bool,
    pub restart: bool,
    pub elf_file: Option<PathBuf>,
    pub command_data_addr: Option<u64>,
    pub command_len_addr: Option<u64>,
    pub stimulus_port: u8,
    pub probe_selector: Option<ProbeSelector>,
    pub chip: Option<String>,
    pub protocol: probe_rs::probe::WireProtocol,
    pub speed: u32,
    pub core: usize,
    pub clk: Option<u32>,
    pub baud: Option<u32>,
    pub reset: bool,
    pub chip_description_path: Option<PathBuf>,
}

impl ItmCollectorConfig {
    pub const DEFAULT_STIMULUS_PORT: u8 = 1;
    pub const DEFAULT_PROTOCOL: probe_rs::probe::WireProtocol = probe_rs::probe::WireProtocol::Swd;
    pub const DEFAULT_SPEED: u32 = 4000;
    pub const DEFAULT_CORE: usize = 0;
}

impl Default for ItmCollectorConfig {
    fn default() -> Self {
        Self {
            disable_control_plane: false,
            restart: false,
            elf_file: None,
            command_data_addr: None,
            command_len_addr: None,
            stimulus_port: Self::DEFAULT_STIMULUS_PORT,
            probe_selector: None,
            chip: None,
            protocol: Self::DEFAULT_PROTOCOL,
            speed: Self::DEFAULT_SPEED,
            core: Self::DEFAULT_CORE,
            clk: None,
            baud: None,
            reset: false,
            chip_description_path: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct RttCollectorConfig {
    pub attach_timeout: Option<HumanTime>,
    pub control_block_address: Option<u32>,
    pub disable_control_plane: bool,
    pub restart: bool,
    pub up_channel: usize,
    pub down_channel: usize,
    pub probe_selector: Option<ProbeSelector>,
    pub chip: Option<String>,
    pub protocol: probe_rs::probe::WireProtocol,
    pub speed: u32,
    pub core: usize,
    pub reset: bool,
    pub attach_under_reset: bool,
    pub chip_description_path: Option<PathBuf>,
    pub rtt_read_buffer_size: Option<usize>,
    pub rtt_poll_interval: Option<HumanTime>,
    pub metrics: bool,
}

impl RttCollectorConfig {
    pub const DEFAULT_UP_CHANNEL: usize = 1;
    pub const DEFAULT_DOWN_CHANNEL: usize = 1;
    pub const DEFAULT_PROTOCOL: probe_rs::probe::WireProtocol = probe_rs::probe::WireProtocol::Swd;
    pub const DEFAULT_SPEED: u32 = 4000;
    pub const DEFAULT_CORE: usize = 0;
}

impl Default for RttCollectorConfig {
    fn default() -> Self {
        Self {
            attach_timeout: None,
            control_block_address: None,
            disable_control_plane: false,
            restart: false,
            up_channel: Self::DEFAULT_UP_CHANNEL,
            down_channel: Self::DEFAULT_DOWN_CHANNEL,
            probe_selector: None,
            chip: None,
            protocol: Self::DEFAULT_PROTOCOL,
            speed: Self::DEFAULT_SPEED,
            core: Self::DEFAULT_CORE,
            reset: false,
            attach_under_reset: false,
            chip_description_path: None,
            rtt_read_buffer_size: None,
            rtt_poll_interval: None,
            metrics: false,
        }
    }
}

#[derive(Clone, Debug, From, Into, Deref, serde_with::DeserializeFromStr)]
pub struct ProbeSelector(pub probe_rs::probe::DebugProbeSelector);

impl PartialEq for ProbeSelector {
    fn eq(&self, other: &Self) -> bool {
        self.0.vendor_id == other.0.vendor_id
            && self.0.product_id == other.0.product_id
            && self.0.serial_number == other.0.serial_number
    }
}

impl Eq for ProbeSelector {}

impl FromStr for ProbeSelector {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(
            probe_rs::probe::DebugProbeSelector::from_str(s).map_err(|e| e.to_string())?,
        ))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, From, Into, Deref, serde_with::DeserializeFromStr)]
pub struct HumanTime(pub humantime::Duration);

impl FromStr for HumanTime {
    type Err = humantime::DurationError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(humantime::Duration::from_str(s)?))
    }
}

impl TraceRecorderConfig {
    pub fn load_merge_with_opts(
        entry: TraceRecorderConfigEntry,
        rf_opts: ReflectorOpts,
        tr_opts: TraceRecorderOpts,
        ignore_env: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let cfg = if let Some(cfg_path) = &rf_opts.config_file {
            auxon_sdk::reflector_config::try_from_file(cfg_path)?
        } else if let Ok(env_path) = env::var(CONFIG_ENV_VAR) {
            if ignore_env {
                Config::default()
            } else {
                auxon_sdk::reflector_config::try_from_file(Path::new(&env_path))?
            }
        } else {
            Config::default()
        };

        let mut ingest = cfg.ingest.clone().unwrap_or_default();
        if let Some(url) = &rf_opts.protocol_parent_url {
            ingest.protocol_parent_url = Some(url.clone());
        }
        if rf_opts.allow_insecure_tls {
            ingest.allow_insecure_tls = true;
        }

        let cfg_plugin = PluginConfig::from_metadata(&cfg, entry)?;
        let plugin = PluginConfig {
            run_id: rf_opts.run_id.or(cfg_plugin.run_id),
            time_domain: rf_opts.time_domain.or(cfg_plugin.time_domain),
            startup_task_name: tr_opts.startup_task_name.or(cfg_plugin.startup_task_name),
            single_task_timeline: if tr_opts.single_task_timeline {
                true
            } else {
                cfg_plugin.single_task_timeline
            },
            flatten_isr_timelines: if tr_opts.flatten_isr_timelines {
                true
            } else {
                cfg_plugin.flatten_isr_timelines
            },
            disable_task_interactions: if tr_opts.disable_task_interactions {
                true
            } else {
                cfg_plugin.disable_task_interactions
            },
            use_timeline_id_channel: if tr_opts.use_timeline_id_channel {
                true
            } else {
                cfg_plugin.use_timeline_id_channel
            },
            include_unknown_events: if tr_opts.include_unknown_events {
                true
            } else {
                cfg_plugin.include_unknown_events
            },
            deviant_event_id_base: tr_opts
                .deviant_event_id_base
                .or(cfg_plugin.deviant_event_id_base),
            ignored_object_classes: if !tr_opts.ignore_object_class.is_empty() {
                tr_opts.ignore_object_class.clone().into_iter().collect()
            } else {
                cfg_plugin.ignored_object_classes
            },
            user_event_channel: if tr_opts.user_event_channel {
                true
            } else {
                cfg_plugin.user_event_channel
            },
            user_event_format_string: if tr_opts.user_event_format_string {
                true
            } else {
                cfg_plugin.user_event_format_string
            },
            user_event_channel_rename_map: if !tr_opts.user_event_channel_name.is_empty() {
                tr_opts
                    .user_event_channel_name
                    .clone()
                    .into_iter()
                    .collect()
            } else {
                cfg_plugin.user_event_channel_rename_map
            },
            user_event_formatted_string_rename_map: if !tr_opts
                .user_event_formatted_string_name
                .is_empty()
            {
                tr_opts
                    .user_event_formatted_string_name
                    .clone()
                    .into_iter()
                    .collect()
            } else {
                cfg_plugin.user_event_formatted_string_rename_map
            },
            user_event_fmt_arg_attr_keys: if !tr_opts.user_event_fmt_arg_attr_keys.is_empty() {
                tr_opts.user_event_fmt_arg_attr_keys.into_iter().collect()
            } else {
                cfg_plugin.user_event_fmt_arg_attr_keys
            },
            interaction_mode: if let Some(m) = tr_opts.interaction_mode {
                m
            } else {
                cfg_plugin.interaction_mode
            },
            import: cfg_plugin.import,
            tcp_collector: cfg_plugin.tcp_collector,
            itm_collector: cfg_plugin.itm_collector,
            rtt_collector: cfg_plugin.rtt_collector,
        };

        Ok(Self {
            auth_token: rf_opts.auth_token,
            ingest,
            plugin,
        })
    }

    pub fn protocol_parent_url(&self) -> Result<Url, url::ParseError> {
        if let Some(url) = &self.ingest.protocol_parent_url {
            Ok(url.clone())
        } else {
            let url = Url::parse("modality-ingest://127.0.0.1:14188")?;
            Ok(url)
        }
    }

    pub fn resolve_auth(&self) -> Result<AuthToken, AuthTokenError> {
        if let Some(auth_token_hex) = self.auth_token.as_deref() {
            Ok(auxon_sdk::auth_token::decode_auth_token_hex(
                auth_token_hex,
            )?)
        } else {
            Ok(AuthToken::load()?)
        }
    }
}

mod internal {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "kebab-case", default)]
    pub struct CommonPluginConfig {
        pub run_id: Option<Uuid>,
        pub time_domain: Option<Uuid>,
        pub startup_task_name: Option<String>,
        pub single_task_timeline: bool,
        pub flatten_isr_timelines: bool,
        pub disable_task_interactions: bool,
        pub use_timeline_id_channel: bool,
        pub include_unknown_events: bool,
        pub deviant_event_id_base: Option<u16>,
        pub ignored_object_classes: IgnoredObjectClasses,
        pub user_event_channel: bool,
        pub user_event_format_string: bool,
        #[serde(rename = "user-event-channel-name")]
        pub user_event_channel_rename_map: RenameMap,
        #[serde(rename = "user-event-formatted-string-name")]
        pub user_event_formatted_string_rename_map: RenameMap,
        pub user_event_fmt_arg_attr_keys: FormatArgAttributeKeysSet,
        pub interaction_mode: InteractionMode,
    }

    impl From<CommonPluginConfig> for PluginConfig {
        fn from(c: CommonPluginConfig) -> Self {
            Self {
                run_id: c.run_id,
                time_domain: c.time_domain,
                startup_task_name: c.startup_task_name,
                single_task_timeline: c.single_task_timeline,
                flatten_isr_timelines: c.flatten_isr_timelines,
                disable_task_interactions: c.disable_task_interactions,
                use_timeline_id_channel: c.use_timeline_id_channel,
                include_unknown_events: c.include_unknown_events,
                deviant_event_id_base: c.deviant_event_id_base,
                ignored_object_classes: c.ignored_object_classes,
                user_event_channel: c.user_event_channel,
                user_event_format_string: c.user_event_format_string,
                user_event_channel_rename_map: c.user_event_channel_rename_map,
                user_event_formatted_string_rename_map: c.user_event_formatted_string_rename_map,
                user_event_fmt_arg_attr_keys: c.user_event_fmt_arg_attr_keys,
                interaction_mode: c.interaction_mode,
                import: Default::default(),
                tcp_collector: Default::default(),
                itm_collector: Default::default(),
                rtt_collector: Default::default(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "kebab-case", default)]
    pub struct ImportPluginConfig {
        #[serde(flatten)]
        pub common: CommonPluginConfig,
        #[serde(flatten)]
        pub import: ImportConfig,
    }

    impl From<ImportPluginConfig> for PluginConfig {
        fn from(pc: ImportPluginConfig) -> Self {
            let ImportPluginConfig { common, import } = pc;
            let mut c = PluginConfig::from(common);
            c.import = import;
            c
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "kebab-case", default)]
    pub struct TcpCollectorPluginConfig {
        #[serde(flatten)]
        pub common: CommonPluginConfig,
        #[serde(flatten)]
        pub tcp_collector: TcpCollectorConfig,
    }

    impl From<TcpCollectorPluginConfig> for PluginConfig {
        fn from(pc: TcpCollectorPluginConfig) -> Self {
            let TcpCollectorPluginConfig {
                common,
                tcp_collector,
            } = pc;
            let mut c = PluginConfig::from(common);
            c.tcp_collector = tcp_collector;
            c
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "kebab-case", default)]
    pub struct ItmCollectorPluginConfig {
        #[serde(flatten)]
        pub common: CommonPluginConfig,
        #[serde(flatten)]
        pub itm_collector: ItmCollectorConfig,
    }

    impl From<ItmCollectorPluginConfig> for PluginConfig {
        fn from(pc: ItmCollectorPluginConfig) -> Self {
            let ItmCollectorPluginConfig {
                common,
                itm_collector,
            } = pc;
            let mut c = PluginConfig::from(common);
            c.itm_collector = itm_collector;
            c
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "kebab-case", default)]
    pub struct RttCollectorPluginConfig {
        #[serde(flatten)]
        pub common: CommonPluginConfig,
        #[serde(flatten)]
        pub rtt_collector: RttCollectorConfig,
    }

    impl From<RttCollectorPluginConfig> for PluginConfig {
        fn from(pc: RttCollectorPluginConfig) -> Self {
            let RttCollectorPluginConfig {
                common,
                rtt_collector,
            } = pc;
            let mut c = PluginConfig::from(common);
            c.rtt_collector = rtt_collector;
            c
        }
    }
}

impl PluginConfig {
    pub(crate) fn from_metadata(
        cfg: &Config,
        entry: TraceRecorderConfigEntry,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use internal::{
            ImportPluginConfig, ItmCollectorPluginConfig, RttCollectorPluginConfig,
            TcpCollectorPluginConfig,
        };
        match entry {
            TraceRecorderConfigEntry::Importer => {
                Self::from_cfg_metadata::<ImportPluginConfig>(cfg).map(|c| c.into())
            }
            TraceRecorderConfigEntry::TcpCollector => {
                Self::from_cfg_metadata::<TcpCollectorPluginConfig>(cfg).map(|c| c.into())
            }
            TraceRecorderConfigEntry::ItmCollector => {
                Self::from_cfg_metadata::<ItmCollectorPluginConfig>(cfg).map(|c| c.into())
            }
            TraceRecorderConfigEntry::RttCollector => {
                Self::from_cfg_metadata::<RttCollectorPluginConfig>(cfg).map(|c| c.into())
            }
        }
    }

    fn from_cfg_metadata<'a, T: Deserialize<'a>>(
        cfg: &Config,
    ) -> Result<T, Box<dyn std::error::Error>> {
        let cfg = TomlValue::Table(cfg.metadata.clone().into_iter().collect()).try_into()?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::opts::{FormatArgAttributeKeysItem, RenameMapItem};
    use auxon_sdk::reflector_config::{AttrKeyEqValuePair, TimelineAttributes};
    use pretty_assertions::assert_eq;
    use std::{env, fs::File, io::Write};
    use trace_recorder_parser::types::ObjectClass;

    const IMPORT_CONFIG: &str = r#"[ingest]
protocol-parent-url = 'modality-ingest://127.0.0.1:14182'
additional-timeline-attributes = [
    "ci_run=1",
    "platform='FreeRTOS'",
    "module='m3'",
    "trc-mode='snapshot'",
]

[metadata]
run-id = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d1'
time-domain = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d1'
startup-task-name = 'm3'
user-event-channel = true
user-event-format-string = true
interaction-mode = "ipc"
single-task-timeline = true
flatten-isr-timelines = true
disable-task-interactions = true
include-unknown-events = true
file = '/path/to/memdump.bin'

    [[metadata.user-event-fmt-arg-attr-keys]]
    channel = 'stats'
    format-string = '%s %u %d %u %u'
    attribute-keys = ['task', 'stack_size', 'stack_high_water', 'task_run_time', 'total_run_time']

    [[metadata.user-event-channel-name]]
    channel = 'act-cmd'
    event-name = 'MY_EVENT'

    [[metadata.user-event-formatted-string-name]]
    formatted-string = 'found 1 thing'
    event-name = 'MY_EVENT2'
"#;

    const TCP_COLLECTOR_CONFIG: &str = r#"[ingest]
protocol-parent-url = 'modality-ingest://127.0.0.1:14182'
additional-timeline-attributes = [
    "ci_run=1",
    "platform='FreeRTOS'",
    "module='m3'",
    "trc-mode='tcp'",
]

[metadata]
run-id = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d2'
time-domain = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d2'
startup-task-name = 'm4'
user-event-channel = true
user-event-format-string = true
single-task-timeline = true
flatten-isr-timelines = true
interaction-mode = "ipc"
include-unknown-events = true
disable-task-interactions = true
disable-control-plane = true
restart = true
connect-timeout = "100ms"
remote = "127.0.0.1:8888"

    [[metadata.user-event-fmt-arg-attr-keys]]
    channel = 'stats'
    format-string = '%s %u %d %u %u'
    attribute-keys = ['task', 'stack_size', 'stack_high_water', 'task_run_time', 'total_run_time']

    [[metadata.user-event-channel-name]]
    channel = 'act-cmd'
    event-name = 'MY_EVENT'

    [[metadata.user-event-formatted-string-name]]
    formatted-string = 'found 1 thing'
    event-name = 'MY_EVENT2'
"#;

    const ITM_COLLECTOR_CONFIG: &str = r#"[ingest]
protocol-parent-url = 'modality-ingest://127.0.0.1:14182'
additional-timeline-attributes = [
    "ci_run=1",
    "platform='FreeRTOS'",
    "module='m3'",
    "trc-mode='itm'",
]

[metadata]
run-id = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3'
time-domain = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3'
startup-task-name = 'm5'
user-event-channel = true
user-event-format-string = true
single-task-timeline = true
flatten-isr-timelines = true
disable-control-plane = true
use-timeline-id-channel = true
interaction-mode = "fully-linearized"
include-unknown-events = true
disable-task-interactions = true
ignored-object-classes = ['queue', 'Semaphore']
restart = true
elf-file = '/path/to/elf.elf'
command-data-addr = 1234
command-len-addr = 3345
stimulus-port = 3
probe-selector = '234:234'
chip = 'stm32'
protocol = 'Jtag'
speed = 1234
core = 1
clk = 222
baud = 4444
reset = true
chip-description-path = "/tmp/S32K_Series.yaml"

    [[metadata.user-event-fmt-arg-attr-keys]]
    channel = 'stats'
    format-string = '%s %u %d %u %u'
    attribute-keys = ['task', 'stack_size', 'stack_high_water', 'task_run_time', 'total_run_time']

    [[metadata.user-event-channel-name]]
    channel = 'act-cmd'
    event-name = 'MY_EVENT'

    [[metadata.user-event-formatted-string-name]]
    formatted-string = 'found 1 thing'
    event-name = 'MY_EVENT2'
"#;

    const RTT_COLLECTOR_CONFIG: &str = r#"[ingest]
protocol-parent-url = 'modality-ingest://127.0.0.1:14182'
additional-timeline-attributes = [
    "ci_run=1",
    "platform='FreeRTOS'",
    "module='m3'",
    "trc-mode='rtt'",
]

[metadata]
run-id = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3'
time-domain = 'a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3'
startup-task-name = 'm5'
control-block-address = 0xFFFF
user-event-channel = true
user-event-format-string = true
single-task-timeline = true
include-unknown-events = true
flatten-isr-timelines = true
disable-control-plane = true
use-timeline-id-channel = true
disable-task-interactions = true
interaction-mode = "ipc"
ignored-object-classes = ['queue', 'Semaphore']
attach-timeout = "100ms"
restart = true
up-channel = 1
down-channel = 1
probe-selector = '234:234'
chip = 'stm32'
protocol = 'Jtag'
speed = 1234
core = 1
reset = true
attach-under-reset = true
chip-description-path = "/tmp/S32K_Series.yaml"
rtt-poll-interval = "1ms"
rtt-read-buffer-size = 1024
metrics = true

    [[metadata.user-event-fmt-arg-attr-keys]]
    channel = 'stats'
    format-string = '%s %u %d %u %u'
    attribute-keys = ['task', 'stack_size', 'stack_high_water', 'task_run_time', 'total_run_time']

    [[metadata.user-event-channel-name]]
    channel = 'act-cmd'
    event-name = 'MY_EVENT'

    [[metadata.user-event-formatted-string-name]]
    formatted-string = 'found 1 thing'
    event-name = 'MY_EVENT2'
"#;

    #[test]
    fn import_cfg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my_config.toml");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(IMPORT_CONFIG.as_bytes()).unwrap();
            f.flush().unwrap();
        }

        env::remove_var(CONFIG_ENV_VAR);
        let cfg = TraceRecorderConfig::load_merge_with_opts(
            TraceRecorderConfigEntry::Importer,
            ReflectorOpts {
                config_file: Some(path.to_path_buf()),
                ..Default::default()
            },
            Default::default(),
            false,
        )
        .unwrap();

        env::set_var(CONFIG_ENV_VAR, path);
        let env_cfg = TraceRecorderConfig::load_merge_with_opts(
            TraceRecorderConfigEntry::Importer,
            Default::default(),
            Default::default(),
            false,
        )
        .unwrap();
        env::remove_var(CONFIG_ENV_VAR);
        assert_eq!(cfg, env_cfg);

        assert_eq!(
            cfg,
            TraceRecorderConfig {
                auth_token: None,
                ingest: TopLevelIngest {
                    protocol_parent_url: Url::parse("modality-ingest://127.0.0.1:14182")
                        .unwrap()
                        .into(),
                    allow_insecure_tls: false,
                    protocol_child_port: None,
                    timeline_attributes: TimelineAttributes {
                        additional_timeline_attributes: vec![
                            AttrKeyEqValuePair::from_str("ci_run=1").unwrap(),
                            AttrKeyEqValuePair::from_str("platform='FreeRTOS'").unwrap(),
                            AttrKeyEqValuePair::from_str("module='m3'").unwrap(),
                            AttrKeyEqValuePair::from_str("trc-mode='snapshot'").unwrap(),
                        ],
                        override_timeline_attributes: Default::default(),
                    },
                    max_write_batch_staleness: None,
                },
                plugin: PluginConfig {
                    run_id: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d1")
                        .unwrap()
                        .into(),
                    time_domain: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d1")
                        .unwrap()
                        .into(),
                    startup_task_name: "m3".to_owned().into(),
                    single_task_timeline: true,
                    flatten_isr_timelines: true,
                    disable_task_interactions: true,
                    use_timeline_id_channel: false,
                    deviant_event_id_base: None,
                    include_unknown_events: true,
                    ignored_object_classes: Default::default(),
                    user_event_channel: true,
                    user_event_format_string: true,
                    user_event_channel_rename_map: vec![RenameMapItem {
                        input: "act-cmd".to_owned(),
                        event_name: "MY_EVENT".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_formatted_string_rename_map: vec![RenameMapItem {
                        input: "found 1 thing".to_owned(),
                        event_name: "MY_EVENT2".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_fmt_arg_attr_keys: vec![FormatArgAttributeKeysItem {
                        channel: "stats".to_owned(),
                        format_string: "%s %u %d %u %u".to_owned(),
                        arg_attr_keys: vec![
                            "task".to_owned(),
                            "stack_size".to_owned(),
                            "stack_high_water".to_owned(),
                            "task_run_time".to_owned(),
                            "total_run_time".to_owned()
                        ],
                    }]
                    .into_iter()
                    .collect(),
                    interaction_mode: InteractionMode::Ipc,
                    import: ImportConfig {
                        protocol: None,
                        file: PathBuf::from("/path/to/memdump.bin").into(),
                    },
                    tcp_collector: Default::default(),
                    itm_collector: Default::default(),
                    rtt_collector: Default::default(),
                },
            }
        );
    }

    #[test]
    fn tcp_collector_cfg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my_config.toml");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(TCP_COLLECTOR_CONFIG.as_bytes()).unwrap();
            f.flush().unwrap();
        }

        let cfg = TraceRecorderConfig::load_merge_with_opts(
            TraceRecorderConfigEntry::TcpCollector,
            ReflectorOpts {
                config_file: Some(path.to_path_buf()),
                ..Default::default()
            },
            Default::default(),
            true,
        )
        .unwrap();

        assert_eq!(
            cfg,
            TraceRecorderConfig {
                auth_token: None,
                ingest: TopLevelIngest {
                    protocol_parent_url: Url::parse("modality-ingest://127.0.0.1:14182")
                        .unwrap()
                        .into(),
                    allow_insecure_tls: false,
                    protocol_child_port: None,
                    timeline_attributes: TimelineAttributes {
                        additional_timeline_attributes: vec![
                            AttrKeyEqValuePair::from_str("ci_run=1").unwrap(),
                            AttrKeyEqValuePair::from_str("platform='FreeRTOS'").unwrap(),
                            AttrKeyEqValuePair::from_str("module='m3'").unwrap(),
                            AttrKeyEqValuePair::from_str("trc-mode='tcp'").unwrap(),
                        ],
                        override_timeline_attributes: Default::default(),
                    },
                    max_write_batch_staleness: None,
                },
                plugin: PluginConfig {
                    run_id: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d2")
                        .unwrap()
                        .into(),
                    time_domain: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d2")
                        .unwrap()
                        .into(),
                    startup_task_name: "m4".to_owned().into(),
                    single_task_timeline: true,
                    flatten_isr_timelines: true,
                    disable_task_interactions: true,
                    use_timeline_id_channel: false,
                    deviant_event_id_base: None,
                    include_unknown_events: true,
                    ignored_object_classes: Default::default(),
                    user_event_channel: true,
                    user_event_format_string: true,
                    user_event_channel_rename_map: vec![RenameMapItem {
                        input: "act-cmd".to_owned(),
                        event_name: "MY_EVENT".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_formatted_string_rename_map: vec![RenameMapItem {
                        input: "found 1 thing".to_owned(),
                        event_name: "MY_EVENT2".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_fmt_arg_attr_keys: vec![FormatArgAttributeKeysItem {
                        channel: "stats".to_owned(),
                        format_string: "%s %u %d %u %u".to_owned(),
                        arg_attr_keys: vec![
                            "task".to_owned(),
                            "stack_size".to_owned(),
                            "stack_high_water".to_owned(),
                            "task_run_time".to_owned(),
                            "total_run_time".to_owned()
                        ],
                    }]
                    .into_iter()
                    .collect(),
                    interaction_mode: InteractionMode::Ipc,
                    import: Default::default(),
                    tcp_collector: TcpCollectorConfig {
                        disable_control_plane: true,
                        restart: true,
                        connect_timeout: HumanTime::from_str("100ms").unwrap().into(),
                        remote: "127.0.0.1:8888".parse::<SocketAddr>().unwrap().into(),
                    },
                    itm_collector: Default::default(),
                    rtt_collector: Default::default(),
                },
            }
        );
    }

    #[test]
    fn itm_collector_cfg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my_config.toml");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(ITM_COLLECTOR_CONFIG.as_bytes()).unwrap();
            f.flush().unwrap();
        }

        let cfg = TraceRecorderConfig::load_merge_with_opts(
            TraceRecorderConfigEntry::ItmCollector,
            ReflectorOpts {
                config_file: Some(path.to_path_buf()),
                ..Default::default()
            },
            Default::default(),
            true,
        )
        .unwrap();

        assert_eq!(
            cfg,
            TraceRecorderConfig {
                auth_token: None,
                ingest: TopLevelIngest {
                    protocol_parent_url: Url::parse("modality-ingest://127.0.0.1:14182")
                        .unwrap()
                        .into(),
                    allow_insecure_tls: false,
                    protocol_child_port: None,
                    timeline_attributes: TimelineAttributes {
                        additional_timeline_attributes: vec![
                            AttrKeyEqValuePair::from_str("ci_run=1").unwrap(),
                            AttrKeyEqValuePair::from_str("platform='FreeRTOS'").unwrap(),
                            AttrKeyEqValuePair::from_str("module='m3'").unwrap(),
                            AttrKeyEqValuePair::from_str("trc-mode='itm'").unwrap(),
                        ],
                        override_timeline_attributes: Default::default(),
                    },
                    max_write_batch_staleness: None,
                },
                plugin: PluginConfig {
                    run_id: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3")
                        .unwrap()
                        .into(),
                    time_domain: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3")
                        .unwrap()
                        .into(),
                    startup_task_name: "m5".to_owned().into(),
                    single_task_timeline: true,
                    flatten_isr_timelines: true,
                    disable_task_interactions: true,
                    use_timeline_id_channel: true,
                    deviant_event_id_base: None,
                    include_unknown_events: true,
                    ignored_object_classes: vec![ObjectClass::Queue, ObjectClass::Semaphore]
                        .into_iter()
                        .collect(),
                    user_event_channel: true,
                    user_event_format_string: true,
                    user_event_channel_rename_map: vec![RenameMapItem {
                        input: "act-cmd".to_owned(),
                        event_name: "MY_EVENT".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_formatted_string_rename_map: vec![RenameMapItem {
                        input: "found 1 thing".to_owned(),
                        event_name: "MY_EVENT2".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_fmt_arg_attr_keys: vec![FormatArgAttributeKeysItem {
                        channel: "stats".to_owned(),
                        format_string: "%s %u %d %u %u".to_owned(),
                        arg_attr_keys: vec![
                            "task".to_owned(),
                            "stack_size".to_owned(),
                            "stack_high_water".to_owned(),
                            "task_run_time".to_owned(),
                            "total_run_time".to_owned()
                        ],
                    }]
                    .into_iter()
                    .collect(),
                    interaction_mode: InteractionMode::FullyLinearized,
                    import: Default::default(),
                    tcp_collector: Default::default(),
                    itm_collector: ItmCollectorConfig {
                        disable_control_plane: true,
                        restart: true,
                        elf_file: PathBuf::from("/path/to/elf.elf").into(),
                        command_data_addr: 1234.into(),
                        command_len_addr: 3345.into(),
                        stimulus_port: 3,
                        probe_selector: ProbeSelector::from_str("234:234").unwrap().into(),
                        chip: "stm32".to_owned().into(),
                        protocol: probe_rs::probe::WireProtocol::Jtag,
                        speed: 1234,
                        core: 1,
                        clk: 222.into(),
                        baud: 4444.into(),
                        reset: true,
                        chip_description_path: PathBuf::from("/tmp/S32K_Series.yaml").into(),
                    },
                    rtt_collector: Default::default(),
                },
            }
        );
    }

    #[test]
    fn rtt_collector_cfg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my_config.toml");
        {
            let mut f = File::create(&path).unwrap();
            f.write_all(RTT_COLLECTOR_CONFIG.as_bytes()).unwrap();
            f.flush().unwrap();
        }

        let cfg = TraceRecorderConfig::load_merge_with_opts(
            TraceRecorderConfigEntry::RttCollector,
            ReflectorOpts {
                config_file: Some(path.to_path_buf()),
                ..Default::default()
            },
            Default::default(),
            true,
        )
        .unwrap();

        assert_eq!(
            cfg,
            TraceRecorderConfig {
                auth_token: None,
                ingest: TopLevelIngest {
                    protocol_parent_url: Url::parse("modality-ingest://127.0.0.1:14182")
                        .unwrap()
                        .into(),
                    allow_insecure_tls: false,
                    protocol_child_port: None,
                    timeline_attributes: TimelineAttributes {
                        additional_timeline_attributes: vec![
                            AttrKeyEqValuePair::from_str("ci_run=1").unwrap(),
                            AttrKeyEqValuePair::from_str("platform='FreeRTOS'").unwrap(),
                            AttrKeyEqValuePair::from_str("module='m3'").unwrap(),
                            AttrKeyEqValuePair::from_str("trc-mode='rtt'").unwrap(),
                        ],
                        override_timeline_attributes: Default::default(),
                    },
                    max_write_batch_staleness: None,
                },
                plugin: PluginConfig {
                    run_id: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3")
                        .unwrap()
                        .into(),
                    time_domain: Uuid::from_str("a1a2a3a4b1b2c1c2d1d2d3d4d5d6d7d3")
                        .unwrap()
                        .into(),
                    startup_task_name: "m5".to_owned().into(),
                    single_task_timeline: true,
                    flatten_isr_timelines: true,
                    disable_task_interactions: true,
                    use_timeline_id_channel: true,
                    deviant_event_id_base: None,
                    include_unknown_events: true,
                    ignored_object_classes: vec![ObjectClass::Queue, ObjectClass::Semaphore]
                        .into_iter()
                        .collect(),
                    user_event_channel: true,
                    user_event_format_string: true,
                    user_event_channel_rename_map: vec![RenameMapItem {
                        input: "act-cmd".to_owned(),
                        event_name: "MY_EVENT".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_formatted_string_rename_map: vec![RenameMapItem {
                        input: "found 1 thing".to_owned(),
                        event_name: "MY_EVENT2".to_owned()
                    }]
                    .into_iter()
                    .collect(),
                    user_event_fmt_arg_attr_keys: vec![FormatArgAttributeKeysItem {
                        channel: "stats".to_owned(),
                        format_string: "%s %u %d %u %u".to_owned(),
                        arg_attr_keys: vec![
                            "task".to_owned(),
                            "stack_size".to_owned(),
                            "stack_high_water".to_owned(),
                            "task_run_time".to_owned(),
                            "total_run_time".to_owned()
                        ],
                    }]
                    .into_iter()
                    .collect(),
                    interaction_mode: InteractionMode::Ipc,
                    import: Default::default(),
                    tcp_collector: Default::default(),
                    itm_collector: Default::default(),
                    rtt_collector: RttCollectorConfig {
                        attach_timeout: HumanTime::from_str("100ms").unwrap().into(),
                        control_block_address: 0xFFFF.into(),
                        disable_control_plane: true,
                        restart: true,
                        up_channel: 1,
                        down_channel: 1,
                        probe_selector: ProbeSelector::from_str("234:234").unwrap().into(),
                        chip: "stm32".to_owned().into(),
                        protocol: probe_rs::probe::WireProtocol::Jtag,
                        speed: 1234,
                        core: 1,
                        reset: true,
                        attach_under_reset: true,
                        chip_description_path: PathBuf::from("/tmp/S32K_Series.yaml").into(),
                        rtt_poll_interval: HumanTime::from_str("1ms").unwrap().into(),
                        rtt_read_buffer_size: 1024.into(),
                        metrics: true,
                    },
                },
            }
        );
    }
}
