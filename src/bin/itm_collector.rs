#![deny(warnings, clippy::all)]

use clap::Parser;
use goblin::elf::Elf;
use itm::{DecoderError, Singles, TracePacket};
use modality_trace_recorder_plugin::{
    import, import::import_streaming, streaming::Command, tracing::try_init_tracing_subscriber,
    Interruptor, ReflectorOpts, TraceRecorderConfig, TraceRecorderOpts,
};
use probe_rs::{
    architecture::arm::{component::TraceSink, SwoConfig},
    DebugProbeSelector, MemoryInterface, Permissions, Probe, WireProtocol,
};
use std::{fs, io, path::PathBuf, time::Duration};
use thiserror::Error;
use tracing::{debug, warn};

/// Collect trace recorder streaming protocol data from ITM packets via SWO
#[derive(Parser, Debug, Clone)]
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
    pub command_data_addr: Option<u32>,

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
    pub command_len_addr: Option<u32>,

    /// The ITM stimulus port used for trace recorder data.
    #[clap(
        long = "stimulus-port",
        name = "stimulus-port",
        default_value = "1",
        help_heading = "STREAMING PORT CONFIGURATION"
    )]
    pub stimulus_port: u8,

    /// Select a specific probe instead of opening the first available one.
    ///
    /// Use '--probe VID:PID' or '--probe VID:PID:Serial' if you have more than one probe with the same VID:PID.
    #[structopt(long = "probe", name = "probe", help_heading = "PROBE CONFIGURATION")]
    pub probe_selector: Option<DebugProbeSelector>,

    /// The target chip to attach to (e.g. STM32F407VE).
    #[clap(long, name = "chip", help_heading = "PROBE CONFIGURATION")]
    pub chip: String,

    /// Protocol used to connect to chip. Possible options: [swd, jtag]
    #[structopt(
        long,
        name = "protocol",
        default_value = "swd",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub protocol: WireProtocol,

    /// The protocol speed in kHz.
    #[clap(
        long,
        name = "speed",
        default_value = "4000",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub speed: u32,

    /// The selected core to target.
    #[clap(
        long,
        name = "core",
        default_value = "0",
        help_heading = "PROBE CONFIGURATION"
    )]
    pub core: usize,

    /// The speed of the clock feeding the TPIU/SWO module in Hz.
    #[clap(long, name = "Hz", help_heading = "PROBE CONFIGURATION")]
    pub clk: u32,

    /// The desired baud rate of the SWO output.
    #[clap(long, name = "baud-rate", help_heading = "PROBE CONFIGURATION")]
    pub baud: u32,

    /// Reset the target on startup.
    #[clap(long, name = "reset", help_heading = "PROBE CONFIGURATION")]
    pub reset: bool,
}

#[tokio::main]
async fn main() {
    match do_main().await {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{}", e);
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
    ctrlc::set_handler(move || {
        if intr.is_set() {
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
            intr.set();
        }
    })?;

    if !opts.disable_control_plane
        && opts.elf_file.is_none()
        && (opts.command_data_addr.is_none() || opts.command_len_addr.is_none())
    {
        return Err(Error::MissingItmPortVariables.into());
    }

    if opts.stimulus_port > 31 {
        return Err(Error::InvalidStimulusPort(opts.stimulus_port).into());
    }

    let control_plane_addrs = if let Some(elf_file) = &opts.elf_file {
        debug!(elf_file = %elf_file.display(), "Reading control plane variable addresses from ELF file");
        let buffer = fs::read(elf_file)?;
        let elf = Elf::parse(&buffer)?;
        Some(TzHostCommandVariables::load_from_elf(&elf)?)
    } else {
        None
    };

    let trc_cfg = TraceRecorderConfig::from((opts.rf_opts, opts.tr_opts));

    let sink_cfg = SwoConfig::new(opts.clk)
        .set_mode_uart()
        .set_baud(opts.baud)
        .set_continuous_formatting(false);

    let mut probe = if let Some(probe_selector) = opts.probe_selector {
        debug!(probe_selector = %probe_selector, "Opening selected probe");
        Probe::open(probe_selector)?
    } else {
        debug!("Opening first available probe");
        let probes = Probe::list_all();
        if probes.is_empty() {
            return Err(Error::NoProbesAvailable.into());
        } else {
            probes[0].open()?
        }
    };

    debug!(protocol = %opts.protocol, speed = opts.speed, "Configuring probe");
    probe.select_protocol(opts.protocol)?;
    probe.set_speed(opts.speed)?;

    if opts.reset {
        debug!("Reseting target");
        probe.target_reset_assert()?;
        probe.target_reset_deassert()?;
    }

    debug!(chip = opts.chip, "Attaching to chip");
    let mut session = probe.attach(opts.chip, Permissions::default())?;

    debug!(
        baud = opts.baud,
        clk = opts.clk,
        "Configuring SWO trace sink"
    );
    session.setup_tracing(opts.core, TraceSink::Swo(sink_cfg))?;

    session.core(0)?.write_word_32(ITM_LAR, ITM_LAR_UNLOCK)?;
    session
        .core(0)?
        .write_word_32(ITM_TER, 1 << opts.stimulus_port)?;

    if let Some(cp_addrs) = control_plane_addrs {
        if opts.restart {
            debug!("Sending stop command");
            let cmd = Command::Stop.to_le_bytes();
            session
                .core(opts.core)?
                .write_8(cp_addrs.tz_host_command_data_addr, &cmd)?;
            session
                .core(opts.core)?
                .write_word_32(cp_addrs.tz_host_command_bytes_to_read_addr, cmd.len() as _)?;
        }

        std::thread::sleep(Duration::from_millis(200));

        debug!("Sending start command");
        let cmd = Command::Start.to_le_bytes();
        session
            .core(opts.core)?
            .write_8(cp_addrs.tz_host_command_data_addr, &cmd)?;
        session
            .core(opts.core)?
            .write_word_32(cp_addrs.tz_host_command_bytes_to_read_addr, cmd.len() as _)?;
    }

    let mut join_handle: tokio::task::JoinHandle<Result<(), Error>> = tokio::spawn(async move {
        let decoder = itm::Decoder::new(
            session.swo_reader()?,
            itm::DecoderOptions { ignore_eof: true },
        );
        let mut stream = TrcItmReader::new(opts.stimulus_port, decoder.singles());
        import_streaming(&mut stream, trc_cfg).await?;
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

    #[error("Encountered an error with the probe. {0}")]
    ProbeRs(#[from] probe_rs::Error),

    #[error(transparent)]
    Import(#[from] import::Error),
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
