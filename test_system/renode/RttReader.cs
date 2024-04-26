//
// Copyright (c) 2024 jon@auxon.io
//
// This file is licensed under the MIT License.
// Full license text is available in 'licenses/MIT.txt'.
//
using System;
using System.Text;

using Antmicro.Migrant;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Time;
using Antmicro.Renode.Exceptions;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Timers;
using Antmicro.Renode.Peripherals.UART;

namespace Antmicro.Renode.Integrations
{
    public static class RttReaderExtensions
    {
        public static void CreateRttReader(this IMachine machine, string name = null)
        {
            var n = name ?? "RttReader";
            var rttReader = new RttReader(machine);

            machine.RegisterAsAChildOf(machine.SystemBus, rttReader, NullRegistrationPoint.Instance);
            machine.SetLocalName(rttReader, n);

            var emulation = EmulationManager.Instance.CurrentEmulation;
            emulation.ExternalsManager.AddExternal(rttReader, n);
        }
    }

    public class RttReader : IExternal, IUART
    {
        public RttReader(IMachine machine)
        {
            this.DebugLog("Creating RttReader");
            this.controlBlockAddress = 0;
            this.machine = machine;
            var frequency = 10; // TODO cfg
            this.pollTimer = new LimitTimer(
                    machine.ClockSource,
                    frequency,
                    null,
                    "rttPollTimer",
                    limit: 1,
                    direction: Direction.Ascending,
                    enabled: false,
                    eventEnabled: true,
                    autoUpdate: true);
            this.pollTimer.LimitReached += TimerLimitReachedCallback;
        }

        public void Start() {
            this.DebugLog("Starting RTT reading");
            pollTimer.Enabled = true;
        }

        public void Stop() {
            this.DebugLog("Stopping RTT reading, flushing");
            pollTimer.Enabled = false;
            if(controlBlockAddress != 0)
            {
                readRttBuffer();
            }
        }

        private void TimerLimitReachedCallback()
        {
            if(controlBlockAddress == 0)
            {
                SearchForControlBlock();
            }
            if(controlBlockAddress != 0)
            {
                readRttBuffer();
            }
        }

        private void SearchForControlBlock() {
            // TODO - the ELF file needs to be loaded
            // or let user provide the address
            // or scan for it
            // or this.machine.SystemBus.GetSymbolAddress
            controlBlockAddress = machine.SystemBus.GetSymbolAddress("_SEGGER_RTT");
            this.DebugLog("Found control block at 0x{0:X}", controlBlockAddress);

            // Read control block
            var cbId = machine.SystemBus.ReadBytes(controlBlockAddress + IdOffset, (int) MaxUpChannelsOffset);
            if(!ByteArraysEqual(cbId, Encoding.ASCII.GetBytes("SEGGER RTT\0\0\0\0\0\0")))
            {
                this.ErrorLog("CB.ID : [{0:X}] ({1})", BitConverter.ToString(cbId), Encoding.ASCII.GetString(cbId));
                throw new RecoverableException("Invalid RTT control block ID");
            }
            var maxUpChannels = machine.SystemBus.ReadDoubleWord(controlBlockAddress + MaxUpChannelsOffset);
            var maxDownChannels = machine.SystemBus.ReadDoubleWord(controlBlockAddress + MaxDownChannelsOffset);
            this.DebugLog("RTT MaxUpChannels={0}, MaxDownChannels={1}", maxUpChannels, maxDownChannels);
            if((maxUpChannels > 255) || (maxDownChannels > 255))
            {
                throw new RecoverableException("RTT up/down channels are invalid");
            }

            // TODO - this is trace-recorder specific
            if((maxUpChannels != 3) || (maxDownChannels != 3))
            {
                throw new RecoverableException("RTT reader only supports channel config up=3 down=3");
            }

            // Read the reset of the control block (just up channel 1)
            var chAddr = controlBlockAddress + ChannelArraysOffset + ChannelSize;
            // Read up channel 1
            // TODO make a type/struct for this
            var chNamePtr = machine.SystemBus.ReadDoubleWord(chAddr + NameOffset);
            chBufferPtr = machine.SystemBus.ReadDoubleWord(chAddr + BufferPtrOffset);
            chSize = machine.SystemBus.ReadDoubleWord(chAddr + SizeOffset);
            chWrite = machine.SystemBus.ReadDoubleWord(chAddr + WriteOffset);
            chRead = machine.SystemBus.ReadDoubleWord(chAddr + ReadOffset);
            var chFlags = machine.SystemBus.ReadDoubleWord(chAddr + FlagsOffset);
            this.DebugLog("RTT channel 1 : name=0x{0:X}, buffer=0x{1:X}, size={2}, write={3}, read={4}, flags=0x{5:X}",
                    chNamePtr, chBufferPtr, chSize, chWrite, chRead, chFlags);

            buffer = new byte[chSize];
        }

        private void readRttBuffer()
        {
            var chAddr = controlBlockAddress + ChannelArraysOffset + ChannelSize; // ch1
            ulong writeAndReadPtrs = machine.SystemBus.ReadQuadWord(chAddr + WriteOffset);
            uint[] writeAndRead = long2doubleUint(writeAndReadPtrs);
            uint write = writeAndRead[0];
            uint read = writeAndRead[1];
            // TODO validate the pointers

            uint total = 0;
            while(total < buffer.Length)
            {
                uint count = Math.Min(readableContiguous(write, read), (uint) buffer.Length);
                if(count == 0)
                {
                    break;
                }

                uint addr = chBufferPtr + read;
                machine.SystemBus.ReadBytes((ulong) addr, (int) count, buffer, (int) total);

                total += count;
                read += count;

                if(read >= chSize)
                {
                    // Wrap around to start
                    read = 0;
                }
            }

            if(total > 0)
            {
                machine.SystemBus.WriteDoubleWord(((ulong) chAddr) + ReadOffset, read);
                this.NoisyLog("RTT bytes {0}", total);
            }

            for(uint i = 0; i < total; i = i + 1)
            {
                CharReceived?.Invoke(buffer[i]);
            }
        }

        public void Reset()
        {
            // Intentionally left empty.
        }

        public void WriteChar(byte value)
        {
            this.WarningLog("RTT reader does not support down-channel IO. Ignoring data 0x{0:X}", value);
        }

        private uint readableContiguous(uint write, uint read)
        {
            if(read > write)
            {
                return chSize - read;
            }
            else
            {
                return write - read;
            }
        }

        private static bool ByteArraysEqual(ReadOnlySpan<byte> a1, ReadOnlySpan<byte> a2)
        {
            return a1.SequenceEqual(a2);
        }

        private static uint[] long2doubleUint(ulong a) {
            uint a1 = (uint)(a & uint.MaxValue);
            uint a2 = (uint)(a >> 32);
            return new uint[] { a1, a2 };
        }

        // TODO add a wrapper ControlBlock struct
        private const ulong MinSize = 24;

        // Offsets of fields in target memory in bytes
        private const ulong IdOffset = 0;
        private const ulong MaxUpChannelsOffset = 16;
        private const ulong MaxDownChannelsOffset = 20;
        private const ulong ChannelArraysOffset = 24;


        // TODO add a wrapper Channel struct
        // Size of the Channel struct in target memory in bytes
        private const ulong ChannelSize = 24;

        // Offsets of fields in target memory in bytes
        private const ulong NameOffset = 0;
        private const ulong BufferPtrOffset = 4;
        private const ulong SizeOffset = 8;
        private const ulong WriteOffset = 12;
        private const ulong ReadOffset = 16;
        private const ulong FlagsOffset = 20;

        private readonly IMachine machine;
        private readonly LimitTimer pollTimer;

        private ulong controlBlockAddress;
        private uint chBufferPtr;
        private uint chSize;
        private uint chRead;
        private uint chWrite;
        private byte[] buffer;

        public uint BaudRate => 0;
        public Parity ParityBit => Parity.None;
        public Bits StopBits => Bits.None;

        [field: Transient]
        public event Action<byte> CharReceived;
    }
}
