use clap::Parser;
use goblin::elf::Elf;
use itm::{DecoderError, Singles, TracePacket};
use modality_trace_recorder_plugin::{
    tracing::try_init_tracing_subscriber, trc_reader, Command, Interruptor, ReflectorOpts,
    TraceRecorderConfig, TraceRecorderConfigEntry, TraceRecorderOpts,
};
use probe_rs::{
    architecture::arm::{component::TraceSink, SwoConfig},
    probe::{list::Lister, DebugProbeSelector, WireProtocol},
    MemoryInterface, Permissions,
};
use std::{fs, io, path::PathBuf, time::Duration};
use thiserror::Error;
use tracing::{debug, warn};

/// Collect trace recorder streaming protocol data from ITM packets via SWO
#[derive(Parser, Debug, Clone)]
#[clap(version)]
struct Opts {
    #[clap(flatten)]
    pub rf_opts: ReflectorOpts,

    #[clap(flatten)]
    pub tr_opts: TraceRecorderOpts,

    /// Disable sending control plane commands to the target.
    /// By default, CMD_SET_ACTIVE is sent on startup and shutdown to
    /// start and stop tracing on the target.
    #[clap(
        long,
        name = "disable-control-plane",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub disable_control_plane: bool,

    /// Send a stop command before a start command to reset tracing on the target.
    #[clap(
        long,
        name = "restart",
        conflicts_with = "disable-control-plane",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub restart: bool,

    /// Extract the location in memory of the ITM streaming port variables from the debug symbols
    /// from an ELF file:
    ///  - tz_host_command_bytes_to_read
    ///  - tz_host_command_data
    ///
    /// These are used to start and stop tracing by writing control plane commands from the probe.
    #[clap(
        long,
        name = "elf-file",
        verbatim_doc_comment,
        conflicts_with = "disable-control-plane",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub elf_file: Option<PathBuf>,

    /// Use the provided memory address for the ITM streaming port variable 'tz_host_command_data'.
    ///
    /// These are used to start and stop tracing by writing control plane commands from the probe.
    #[clap(
        long = "data-addr",
        name = "command-data-addr",
        requires = "command-len-addr",
        conflicts_with_all = &["disable-control-plane", "elf-file"],
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub command_data_addr: Option<u64>,

    /// Use the provided memory address for the ITM streaming port variable 'tz_host_command_bytes_to_read'.
    ///
    /// These are used to start and stop tracing by writing control plane commands from the probe.
    #[clap(
        long = "len-addr",
        name = "command-len-addr",
        requires = "command-data-addr",
        conflicts_with_all = &["disable-control-plane", "elf-file"],
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub command_len_addr: Option<u64>,

    /// The ITM stimulus port used for trace recorder data.
    ///
    /// The default value is 1.
    #[clap(
        long = "stimulus-port",
        name = "stimulus-port",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub stimulus_port: Option<u8>,

    /// Select a specific probe instead of opening the first available one.
    ///
    /// Use '--probe VID:PID' or '--probe VID:PID:Serial' if you have more than one probe with the same VID:PID.
    #[structopt(long = "probe", name = "probe", help_heading = "PROBE CONFIGURATION")]
    pub probe_selector: Option<DebugProbeSelector>,

    /// The target chip to attach to (e.g. STM32F407VE).
    #[clap(long, name = "chip", help_heading = "PROBE CONFIGURATION")]
    pub chip: Option<String>,

    /// Protocol used to connect to chip.
    /// Possible options: [swd, jtag].
    ///
    /// The default value is swd.
    #[structopt(long, name = "protocol", help_heading = "PROBE CONFIGURATION")]
    pub protocol: Option<WireProtocol>,

    /// The protocol speed in kHz.
    ///
    /// The default value is 4000.
    #[clap(long, name = "speed", help_heading = "PROBE CONFIGURATION")]
    pub speed: Option<u32>,

    /// The selected core to target.
    ///
    /// The default value is 0.
    #[clap(long, name = "core", help_heading = "PROBE CONFIGURATION")]
    pub core: Option<usize>,

    /// The speed of the clock feeding the TPIU/SWO module in Hz.
    #[clap(long, name = "Hz", help_heading = "PROBE CONFIGURATION")]
    pub clk: Option<u32>,

    /// The desired baud rate of the SWO output.
    #[clap(long, name = "baud-rate", help_heading = "PROBE CONFIGURATION")]
    pub baud: Option<u32>,

    /// Reset the target on startup.
    #[clap(long, name = "reset", help_heading = "PROBE CONFIGURATION")]
    pub reset: bool,

    /// Chip description YAML file path.
    /// Provides custom target descriptions based on CMSIS Pack files.
    #[clap(
        long,
        name = "chip-description-path",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub chip_description_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    match do_main().await {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{e}");
            let mut cause = e.source();
            while let Some(err) = cause {
                eprintln!("Caused by: {err}");
                cause = err.source();
            }
            std::process::exit(exitcode::SOFTWARE);
        }
    }
}

async fn do_main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();

    try_init_tracing_subscriber()?;

    let intr = Interruptor::new();
    let intr_clone = intr.clone();
    ctrlc::set_handler(move || {
        if intr_clone.is_set() {
            let exit_code = if cfg!(target_family = "unix") {
                // 128 (fatal error signal "n") + 2 (control-c is fatal error signal 2)
                130
            } else {
                // Windows code 3221225786
                // -1073741510 == C000013A
                -1073741510
            };
            std::process::exit(exit_code);
        } else {
            intr_clone.set();
        }
    })?;

    let mut trc_cfg = TraceRecorderConfig::load_merge_with_opts(
        TraceRecorderConfigEntry::ItmCollector,
        opts.rf_opts,
        opts.tr_opts,
        false,
    )?;

    if opts.disable_control_plane {
        trc_cfg.plugin.itm_collector.disable_control_plane = true;
    }
    if opts.restart {
        trc_cfg.plugin.itm_collector.restart = true;
    }
    if let Some(elf) = opts.elf_file {
        trc_cfg.plugin.itm_collector.elf_file = Some(elf);
    }
    if let Some(addr) = opts.command_data_addr {
        trc_cfg.plugin.itm_collector.command_data_addr = Some(addr);
    }
    if let Some(addr) = opts.command_len_addr {
        trc_cfg.plugin.itm_collector.command_len_addr = Some(addr);
    }
    if let Some(sp) = opts.stimulus_port {
        trc_cfg.plugin.itm_collector.stimulus_port = sp;
    }
    if let Some(ps) = &opts.probe_selector {
        trc_cfg.plugin.itm_collector.probe_selector = Some(ps.clone().into());
    }
    if let Some(c) = opts.chip {
        trc_cfg.plugin.rtt_collector.chip = Some(c);
    }
    if let Some(p) = opts.protocol {
        trc_cfg.plugin.itm_collector.protocol = p;
    }
    if let Some(s) = opts.speed {
        trc_cfg.plugin.itm_collector.speed = s;
    }
    if let Some(c) = opts.core {
        trc_cfg.plugin.itm_collector.core = c;
    }
    if let Some(c) = opts.clk {
        trc_cfg.plugin.itm_collector.clk = Some(c);
    }
    if let Some(c) = opts.baud {
        trc_cfg.plugin.itm_collector.baud = Some(c);
    }
    if opts.reset {
        trc_cfg.plugin.itm_collector.reset = true;
    }
    if let Some(cd) = &opts.chip_description_path {
        trc_cfg.plugin.itm_collector.chip_description_path = Some(cd.clone());
    }

    if !trc_cfg.plugin.itm_collector.disable_control_plane
        && trc_cfg.plugin.itm_collector.elf_file.is_none()
        && (trc_cfg.plugin.itm_collector.command_data_addr.is_none()
            || trc_cfg.plugin.itm_collector.command_len_addr.is_none())
    {
        return Err(Error::MissingItmPortVariables.into());
    }

    let clk = trc_cfg.plugin.itm_collector.clk.ok_or(Error::MissingClk)?;
    let baud = trc_cfg
        .plugin
        .itm_collector
        .baud
        .ok_or(Error::MissingBaud)?;
    let chip = trc_cfg
        .plugin
        .itm_collector
        .chip
        .clone()
        .ok_or(Error::MissingChip)?;

    if let Some(chip_desc) = &trc_cfg.plugin.itm_collector.chip_description_path {
        debug!(path = %chip_desc.display(), "Adding custom chip description");
        let f = fs::File::open(chip_desc)?;
        probe_rs::config::add_target_from_yaml(f)?;
    }

    if trc_cfg.plugin.itm_collector.stimulus_port > 31 {
        return Err(Error::InvalidStimulusPort(trc_cfg.plugin.itm_collector.stimulus_port).into());
    }

    let control_plane_addrs = if let Some(elf_file) = &trc_cfg.plugin.itm_collector.elf_file {
        debug!(elf_file = %elf_file.display(), "Reading control plane variable addresses from ELF file");
        let buffer = fs::read(elf_file)?;
        let elf = Elf::parse(&buffer)?;
        Some(TzHostCommandVariables::load_from_elf(&elf)?)
    } else {
        match (
            trc_cfg.plugin.itm_collector.command_data_addr,
            trc_cfg.plugin.itm_collector.command_len_addr,
        ) {
            (Some(data_addr), Some(len_addr)) => Some(TzHostCommandVariables {
                tz_host_command_bytes_to_read_addr: len_addr,
                tz_host_command_data_addr: data_addr,
            }),
            _ => None,
        }
    };

    let sink_cfg = SwoConfig::new(clk)
        .set_mode_uart()
        .set_baud(baud)
        .set_continuous_formatting(false);

    let lister = Lister::new();
    let mut probe = if let Some(probe_selector) = &trc_cfg.plugin.itm_collector.probe_selector {
        debug!(probe_selector = %probe_selector.0, "Opening selected probe");
        lister.open(probe_selector.0.clone())?
    } else {
        debug!("Opening first available probe");
        let probes = lister.list_all();
        if probes.is_empty() {
            return Err(Error::NoProbesAvailable.into());
        }
        probes[0].open()?
    };

    debug!(protocol = %trc_cfg.plugin.itm_collector.protocol, speed = trc_cfg.plugin.itm_collector.speed, "Configuring probe");
    probe.select_protocol(trc_cfg.plugin.itm_collector.protocol)?;
    probe.set_speed(trc_cfg.plugin.itm_collector.speed)?;

    if trc_cfg.plugin.itm_collector.reset {
        debug!("Reseting target");
        probe.target_reset_assert()?;
        probe.target_reset_deassert()?;
    }

    debug!(chip = chip, "Attaching to chip");
    let mut session = probe.attach(chip, Permissions::default())?;

    debug!(baud = baud, clk = clk, "Configuring SWO trace sink");
    session.setup_tracing(trc_cfg.plugin.itm_collector.core, TraceSink::Swo(sink_cfg))?;

    session
        .core(trc_cfg.plugin.itm_collector.core)?
        .write_word_32(ITM_LAR, ITM_LAR_UNLOCK)?;
    session
        .core(trc_cfg.plugin.itm_collector.core)?
        .write_word_32(ITM_TER, 1 << trc_cfg.plugin.itm_collector.stimulus_port)?;

    if let Some(cp_addrs) = control_plane_addrs {
        if trc_cfg.plugin.itm_collector.restart {
            debug!("Sending stop command");
            let cmd = Command::Stop.to_le_bytes();
            session
                .core(trc_cfg.plugin.itm_collector.core)?
                .write_8(cp_addrs.tz_host_command_data_addr, &cmd)?;
            session
                .core(trc_cfg.plugin.itm_collector.core)?
                .write_word_32(cp_addrs.tz_host_command_bytes_to_read_addr, cmd.len() as _)?;
        }

        std::thread::sleep(Duration::from_millis(200));

        debug!("Sending start command");
        let cmd = Command::Start.to_le_bytes();
        session
            .core(trc_cfg.plugin.itm_collector.core)?
            .write_8(cp_addrs.tz_host_command_data_addr, &cmd)?;
        session
            .core(trc_cfg.plugin.itm_collector.core)?
            .write_word_32(cp_addrs.tz_host_command_bytes_to_read_addr, cmd.len() as _)?;
    }

    let mut join_handle: tokio::task::JoinHandle<Result<(), Error>> = tokio::spawn(async move {
        let decoder = itm::Decoder::new(
            session.swo_reader()?,
            itm::DecoderOptions { ignore_eof: true },
        );
        let mut stream = TrcItmReader::new(
            trc_cfg.plugin.itm_collector.stimulus_port,
            decoder.singles(),
        );
        trc_reader::run(&mut stream, trc_cfg, intr).await?;
        Ok(())
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            debug!("User signaled shutdown");
        }
        res = &mut join_handle => {
            match res? {
                Ok(_) => {},
                Err(e) => return Err(e.into()),
            }
        }
    };

    Ok(())
}

#[derive(Debug, Error)]
enum Error {
    #[error("Failed to find symbol '{0}' in ELF file")]
    SymbolNotFound(&'static str),

    #[error("When the control-plane is enabled, the ITM streaming port variable addresses are required. Provide either an ELF file with symbols or manually with '--len-addr' and '--data-addr'")]
    MissingItmPortVariables,

    #[error("No probes available")]
    NoProbesAvailable,

    #[error("Invalid stimulus port {0}. Must be in the range of 0..=31")]
    InvalidStimulusPort(u8),

    #[error("Missing clock speed. Either supply it as a option at the CLI or a config file member 'clk'")]
    MissingClk,

    #[error(
        "Missing baud rate. Either supply it as a option at the CLI or a config file member 'baud'"
    )]
    MissingBaud,

    #[error(
        "Missing chip. Either supply it as a option at the CLI or a config file member 'chip'"
    )]
    MissingChip,

    #[error("Encountered an error with the probe. {0}")]
    ProbeRs(#[from] probe_rs::Error),

    #[error(transparent)]
    TraceRecorder(#[from] modality_trace_recorder_plugin::Error),
}

const ITM_LAR: u64 = 0xE0000FB0;
const ITM_TER: u64 = 0xE0000E00;
const ITM_LAR_UNLOCK: u32 = 0xC5ACCE55;

/// Stores the location in memory of the ITM streaming port variables:
/// ```c
/// volatile int32_t tz_host_command_bytes_to_read = 0;
/// volatile char tz_host_command_data[32];
/// ```
#[derive(Debug, Copy, Clone)]
struct TzHostCommandVariables {
    tz_host_command_bytes_to_read_addr: u64,
    tz_host_command_data_addr: u64,
}

impl TzHostCommandVariables {
    const HOST_CMD_LEN_SYM: &'static str = "tz_host_command_bytes_to_read";
    const HOST_CMD_DATA_SYM: &'static str = "tz_host_command_data";

    fn load_from_elf(elf: &Elf) -> Result<Self, Error> {
        let find_sym_addr = |name| {
            elf.syms.iter().find_map(|sym| {
                if elf.strtab.get_at(sym.st_name) == Some(name) {
                    debug!(
                        symbol = name,
                        address = sym.st_value,
                        "Found debug symbol in ELF file"
                    );
                    sym.st_value.into()
                } else {
                    None
                }
            })
        };

        match (
            find_sym_addr(Self::HOST_CMD_LEN_SYM),
            find_sym_addr(Self::HOST_CMD_DATA_SYM),
        ) {
            (Some(len_addr), Some(data_addr)) => Ok(TzHostCommandVariables {
                tz_host_command_bytes_to_read_addr: len_addr,
                tz_host_command_data_addr: data_addr,
            }),
            (Some(_len_addr), None) => Err(Error::SymbolNotFound(Self::HOST_CMD_DATA_SYM)),
            (None, Some(_data_addr)) => Err(Error::SymbolNotFound(Self::HOST_CMD_LEN_SYM)),
            (None, None) => Err(Error::SymbolNotFound(Self::HOST_CMD_DATA_SYM)),
        }
    }
}

struct TrcItmReader<R: io::Read> {
    port: u8,
    decoder: Singles<R>,
    buf: Vec<u8>,
}

impl<R: io::Read> TrcItmReader<R> {
    pub fn new(port: u8, decoder: Singles<R>) -> Self {
        Self {
            port,
            decoder,
            buf: Vec::new(),
        }
    }

    fn next_instrumentation_payload(&mut self) -> Result<Option<Vec<u8>>, io::Error> {
        match self.decoder.next() {
            None => Ok(None),
            Some(Err(e)) => match e {
                DecoderError::Io(io_err) => Err(io_err),
                DecoderError::MalformedPacket(mp_err) => {
                    warn!("{mp_err}");
                    Ok(None)
                }
            },
            Some(Ok(pkt)) => {
                if let TracePacket::Instrumentation { port, payload } = pkt {
                    if port == self.port {
                        Ok(Some(payload))
                    } else {
                        debug!("Ignoring {} bytes from stimulus port {port}", payload.len());
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }
}

impl<R: io::Read> io::Read for TrcItmReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use std::{cmp, mem};

        while self.buf.len() < buf.len() {
            if let Some(mut payload) = self.next_instrumentation_payload()? {
                self.buf.append(&mut payload);
            }
        }

        let itm = {
            let next_buf = self.buf.split_off(cmp::min(self.buf.len(), buf.len()));
            mem::replace(&mut self.buf, next_buf)
        };

        buf[..itm.len()].copy_from_slice(&itm);
        Ok(itm.len())
    }
}
