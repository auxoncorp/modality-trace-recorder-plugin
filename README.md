# modality-trace-recorder-plugin &emsp; ![ci] [![crates.io]](https://crates.io/crates/modality-trace-recorder-plugin) [![docs.rs]](https://docs.rs/modality-trace-recorder-plugin)

A [Modality][modality] reflector plugin suite and ingest adapter library for Percepio's [TraceRecorder][trace-recorder] data.

| Kernel Port | Snapshot Protocol | Streaming Protocol | File Import | TCP   | ITM   |
| :---:       | :---:             | :---:              | :---:       | :---: | :---: |
| FreeRTOS    | v6                | v6                 | :heavy_check_mark: | :heavy_check_mark: | :heavy_check_mark: |

## Getting Started

1. Add [TraceRecorder][trace-recorder] source files and configuration into your RTOS project (e.g. using [Using FreeRTOS-Plus-Trace](https://www.freertos.org/FreeRTOS-Plus/FreeRTOS_Plus_Trace/RTOS_Trace_Instructions.html))
2. Use the importer to import a memory dump or streaming data file, or use one of the available streaming collectors to collect data from a running system

## Adapter Concept Mapping

The following describes the default mapping between [TraceRecorder's][trace-recorder] concepts
and [Modality's][modality] concepts. See the configuration section for ways to change the
default behavior.

* Task and ISR objects are represented as separate Modality timelines
* The initial startup task ('(startup)') is also represented as a separate Modality timeline
* Streaming header and snapshot fields are represented as Modality timeline attributes under the `timeline.internal.trace_recorder` prefix
* Object properties (i.e. task priority) are represented as Modality event attributes under the `event.internal.trace_recorder` prefix
* User event (`USER_EVENT`) attributes are at the root level (e.g. `event.channel`)
* Task and ISR context switches will be synthesized into Modality timeline interactions

See the [Modality documentation](https://docs.auxon.io/modality/) for more information on the Modality concepts.

## Configuration

All of the plugins can be configured through a TOML configuration file (from either the `--config` option or the `MODALITY_REFLECTOR_CONFIG` environment variable).
All of the configuration fields can optionally be overridden at the CLI, see `--help` for more details.

See the [`modality-reflector` Configuration File documentation](https://docs.auxon.io/modality/ingest/modality-reflector-configuration-file.html) for more information
about the reflector configuration.

### Common Sections

These sections are the same for each of the plugins.

* `[ingest]` — Top-level ingest configuration.
  - `additional-timeline-attributes` — Array of key-value attribute pairs to add to every timeline seen by the plugin.
  - `override-timeline-attributes` — Array of key-value attribute pairs to override on every timeline seen by this plugin.
  - `allow-insecure-tls` — Whether to allow insecure connections. Defaults to `true`.
  - `protocol-parent-url` — URL to which this reflector will send its collected data.

* `[metadata]` — Plugin configuration table.
  - `run-id` — Use the provided UUID as the run ID instead of generating a random one.
  - `time-domain` — Use the provided UUID as the time domain ID instead of generating a random one.
  - `startup-task-name` — Use the provided initial startup task name instead of the default (`(startup)`).
  - `single-task-timeline` — Use a single timeline for all tasks instead of a timeline per task. ISRs can still be represented with their own timelines or not.
  - `disable-task-interactions` — Don't synthesize interactions between tasks and ISRs when a context switch occurs.
  - `user-event-channel` — Instead of `USER_EVENT @ <task-name>`, use the user event channel as the event name (`<channel> @ <task-name>`).
  - `user-event-format-string` — Instead of `USER_EVENT @ <task-name>`, use the user event format string as the event name (`<format-string> @ <task-name>`).
  - `[[user-event-channel-name]]` — Use a custom event name whenever a user event with a matching channel is processed.
    * `channel`— The input channel name to match on.
    * `event-name`— The Modality event name to use.
  - `[[user-event-formatted-string-name]]` — Use a custom event name whenever a user event with a matching formatted string is processed.
    * `formatted-string`— The input formatted string to match on.
    * `event-name`— The Modality event name to use.
  - `[[user-event-fmt-arg-attr-keys]]` — Use custom attribute keys instead of the default `argN` keys for user events matching the given channel and format string.
    * `channel`— The input channel name to match on.
    * `format-string`— The input format string to match on.
    * `attribute-keys`— Array of Modality event attribute keys to use.

### Importer Section

These `metadata` fields are specific to the importer plugin.

Note that individual plugin configuration goes in a specific table in your
reflector configuration file, e.g. `[plugins.ingest.importers.trace-recorder.metadata]`.

* `[metadata]` — Plugin configuration table.
  - `protocol` — The protocol to use. Either `streaming`, `snapshot`, or `auto`. The default is `auto`.
  - `file` — Path to the file to import.

### TCP Collector Section

These `metadata` fields are specific to the streaming TCP collector plugin.

Note that individual plugin configuration goes in a specific table in your
reflector configuration file, e.g. `[plugins.ingest.collectors.trace-recorder-tcp.metadata]`.

* `[metadata]` — Plugin configuration table.
  - `disable-control-plane` — Disable sending control plane commands to the target. By default, `CMD_SET_ACTIVE` is sent on startup and shutdown to start and stop tracing on the target.
  - `restart` — Send a stop command before a start command to reset tracing on the target.
  - `connect-timeout` — Specify a connection timeout. Accepts durations like "10ms" or "1minute 2seconds 22ms".
  - `remote` — The remote address and port to connect to. The default is `127.0.0.1:8888`.

### ITM Collector Section

These `metadata` fields are specific to the streaming ITM collector plugin.

Note that individual plugin configuration goes in a specific table in your
reflector configuration file, e.g. `[plugins.ingest.collectors.trace-recorder-itm.metadata]`.

* `[metadata]` — Plugin configuration table.
  - `disable-control-plane` — Disable sending control plane commands to the target. By default, `CMD_SET_ACTIVE` is sent on startup and shutdown to start and stop tracing on the target.
  - `restart` — Send a stop command before a start command to reset tracing on the target.
  - `elf-file` — Extract the location in memory of the ITM streaming port variables from the debug symbols from an ELF file (`tz_host_command_bytes_to_read` and `tz_host_command_data`).
    These are used to start and stop tracing by writing control plane commands from the probe.
  - `command-data-addr` —  Use the provided memory address for the ITM streaming port variable `tz_host_command_data`.
    These are used to start and stop tracing by writing control plane commands from the probe.
  - `command-len-addr` — Use the provided memory address for the ITM streaming port variable `tz_host_command_bytes_to_read`.
    These are used to start and stop tracing by writing control plane commands from the probe.
  - `stimulus-port` — The ITM stimulus port used for trace recorder data.The default value is 1.
  - `probe-selector` — Select a specific probe instead of opening the first available one.
  - `chip` — The target chip to attach to (e.g. `STM32F407VE`).
  - `protocol` — Protocol used to connect to chip. Possible options: [`swd`, `jtag`]. The default value is `swd`.
  - `speed` — The protocol speed in kHz. The default value is 4000.
  - `core` — The selected core to target. The default value is 0.
  - `clk` — The speed of the clock feeding the TPIU/SWO module in Hz.
  - `baud` — The desired baud rate of the SWO output.
  - `reset` — Reset the target on startup.

## LICENSE

See [LICENSE](./LICENSE) for more details.

Copyright 2022 [Auxon Corporation](https://auxon.io)

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

[http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0)

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

[ci]: https://github.com/auxoncorp/modality-trace-recorder-plugin/workflows/CI/badge.svg
[crates.io]: https://img.shields.io/crates/v/modality-trace-recorder-plugin.svg
[docs.rs]: https://docs.rs/modality-trace-recorder-plugin/badge.svg
[trace-recorder]: https://github.com/percepio/TraceRecorderSource
[modality]: https://auxon.io/products/modality
