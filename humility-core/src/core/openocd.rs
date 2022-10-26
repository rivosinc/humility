// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, bail, ensure, Result};

use crate::regs::Register;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::str;
use std::time::Duration;
use std::time::Instant;

use crate::core::{Core, CORE_MAX_READSIZE};

const OPENOCD_COMMAND_DELIMITER: u8 = 0x1a;
const OPENOCD_TRACE_DATA_BEGIN: &str = "type target_trace data ";
const OPENOCD_TRACE_DATA_END: &str = "\r\n";

pub struct OpenOCDCore {
    stream: TcpStream,
    swv: bool,
    last_swv: Option<Instant>,
    halted: bool,
    was_halted: bool,
}

#[rustfmt::skip::macros(anyhow, bail)]
impl OpenOCDCore {
    pub fn sendcmd(&mut self, cmd: &str) -> Result<String> {
        let mut rbuf = vec![0; 1024];
        let mut result = String::with_capacity(16);

        let mut str = String::from(cmd);
        str.push(OPENOCD_COMMAND_DELIMITER as char);

        self.stream.write_all(str.as_bytes())?;

        loop {
            let rval = self.stream.read(&mut rbuf)?;

            if rbuf[rval - 1] == OPENOCD_COMMAND_DELIMITER {
                result.push_str(str::from_utf8(&rbuf[0..rval - 1])?);
                break;
            }

            result.push_str(str::from_utf8(&rbuf[0..rval])?);
        }

        //
        // Surely not surprisingly, OpenOCD doesn't have a coherent way of
        // indicating that a command has failed.  We fall back to assuming
        // that any return value that contains "Error: " or "invalid command
        // name" is in fact an error.
        //
        if result.contains("Error: ") {
            Err(anyhow!("OpenOCD command \"{}\" failed with \"{}\"", cmd, result))
        } else if result.contains("invalid command name ") {
            Err(anyhow!("OpenOCD command \"{}\" invalid: \"{}\"", cmd, result))
        } else {
            Ok(result)
        }
    }

    pub fn new() -> Result<OpenOCDCore> {
        let addr = "127.0.0.1:6666".parse()?;
        let timeout = Duration::from_millis(100);
        let stream =
            TcpStream::connect_timeout(&addr, timeout).map_err(|_| {
                anyhow!("can't connect to OpenOCD on port 6666; is it running?")
            })?;
        let mut core = Self {
            stream,
            swv: false,
            last_swv: None,
            halted: false,
            was_halted: false,
        };
        // determine if the core is initially halted
        let _target = core.sendcmd("set targ [target current]")?;
        core.halted = match core.sendcmd("$targ curstate")?.as_str() {
            "halted" => {
                log::trace!("connected to halted core");
                true
            }
            "running" => {
                log::trace!("connected to running core");
                false
            }
            _ => {
                crate::msg!("Target in unknown state, humility will leave the core in a running state");
                false
            }
        };
        // if core was initially halted, we want to leave in a halted state after any operation
        core.was_halted = core.halted;
        Ok(core)
    }
}

#[rustfmt::skip::macros(anyhow, bail)]
impl Core for OpenOCDCore {
    fn info(&self) -> (String, Option<String>) {
        ("OpenOCD".to_string(), None)
    }

    fn read_word_32(&mut self, addr: u32) -> Result<u32> {
        self.op_start()?;
        let result = self.sendcmd(&format!("mrw 0x{:x}", addr))?;
        self.op_done()?;
        Ok(result.parse::<u32>()?)
    }

    fn read_8(&mut self, addr: u32, data: &mut [u8]) -> Result<()> {
        ensure!(
            data.len() <= CORE_MAX_READSIZE,
            "read of {} bytes at 0x{:x} exceeds max of {}",
            data.len(),
            addr,
            CORE_MAX_READSIZE
        );
        self.op_start()?;

        //
        // To read an array, we put it in a TCL variable called "output"
        // and then dump the variable.
        //
        let cmd = format!("mem2array output 8 0x{:x} {}", addr, data.len());

        self.sendcmd("array unset output")?;
        self.sendcmd(&cmd)?;

        let mut index = None;
        let mut seen = vec![false; data.len()];

        let result = self.sendcmd("return $output")?;

        //
        // Entirely on-brand, if the mem2array command has failed wildly,
        // OpenOCD won't actually return an error to us -- it will merely
        // fail to set the variable (and we will therefore fail when
        // we attempt to retrieve the variable).  If we fail to
        // retrieve the variable, we infer it to be a failure to
        // perform the read and bail explicitly.
        //
        if result.contains("no such variable") {
            bail!("read at 0x{:x} for {} bytes failed", addr, data.len());
        }

        //
        // The output here is bonkers: instead of being (merely) the array,
        // it's an (undelimited) set of 2-tuples of (index, value) -- sorted
        // in strict alphabetical order by index (!!).  (That is, index 100
        // comes before, say, index 11.)
        //
        for val in result.split(' ') {
            match index {
                None => {
                    let idx = val.parse::<usize>()?;

                    if idx >= data.len() {
                        bail!("\"{}\": illegal index {}", cmd, idx);
                    }

                    if seen[idx] {
                        bail!("\"{}\": duplicate index {}", cmd, idx);
                    }

                    seen[idx] = true;
                    index = Some(idx);
                }

                Some(idx) => {
                    data[idx] = val.parse::<u8>()?;
                    index = None;
                }
            }
        }

        for v in seen.iter().enumerate() {
            ensure!(v.1, "\"{}\": missing index {}", cmd, v.0);
        }
        self.op_done()?;

        Ok(())
    }

    fn write_reg(&mut self, _reg: Register, _val: u32) -> Result<()> {
        // This does not work right now, TODO?
        // openocd does support reading though
        //
        Err(anyhow!(
            "Writing registers is not currently supported with OpenOCD"
        ))
    }

    fn read_reg(&mut self, reg: Register) -> Result<u32> {
        let reg_id = reg.to_gdb_id();

        self.op_start()?;

        let cmd = format!("reg {}", reg_id);
        let rval = self.sendcmd(&cmd)?;

        if let Some(line) = rval.lines().next() {
            if let Some(val) = line.split_whitespace().last() {
                if let Ok(rval) = parse_int::parse::<u32>(val) {
                    return Ok(rval);
                }
            }
        }
        self.op_done()?;

        Err(anyhow!("\"{}\": malformed return value: {:?}", cmd, rval))
    }

    fn init_swv(&mut self) -> Result<()> {
        self.swv = true;
        self.sendcmd("tpiu config disable")?;

        //
        // XXX: This assumes STM32F4's 16Mhz clock
        //
        self.sendcmd("tpiu config internal - uart on 16000000")?;
        self.sendcmd("tcl_trace on")?;

        Ok(())
    }

    fn read_swv(&mut self) -> Result<Vec<u8>> {
        if !self.swv {
            self.init_swv()?
        }

        let mut rbuf = vec![0; 8192];
        let mut swv: Vec<u8> = Vec::with_capacity(8192);

        if let Some(last_swv) = self.last_swv {
            //
            // When we read from SWV from OpenOCD, it will block until data
            // becomes available.  To better approximate the (non-blocking)
            // behavior we see on a directly attached debugger, we return a
            // zero byte read if it has been less than 100 ms since our last
            // read -- relying on OpenOCD to buffer things a bit.
            //
            if last_swv.elapsed().as_secs_f64() < 0.1 {
                return Ok(swv);
            }
        }

        let rval = self.stream.read(&mut rbuf)?;
        self.last_swv = Some(Instant::now());

        if rbuf[rval - 1] != OPENOCD_COMMAND_DELIMITER {
            bail!("missing trace data delimiter: {:?}", rval);
        }

        //
        // OpenOCD can sometimes send multiple command delimters -- or
        // none at all.
        //
        if rval == 1 {
            return Ok(swv);
        }

        let rstr = if rbuf[0] == OPENOCD_COMMAND_DELIMITER {
            str::from_utf8(&rbuf[1..rval - 1])?
        } else {
            str::from_utf8(&rbuf[0..rval - 1])?
        };

        if !rstr.starts_with(OPENOCD_TRACE_DATA_BEGIN) {
            bail!("bogus trace data (bad start): {:?}", rstr);
        }

        if !rstr.ends_with(OPENOCD_TRACE_DATA_END) {
            bail!("bogus trace data (bad end): {:?}", rstr);
        }

        let begin = OPENOCD_TRACE_DATA_BEGIN.len();
        let end = rstr.len() - OPENOCD_TRACE_DATA_END.len();

        for i in (begin..end).step_by(2) {
            if i + 1 >= end {
                bail!("short trace data: {:?}", rval);
            }

            swv.push(u8::from_str_radix(&rstr[i..=i + 1], 16)?);
        }

        Ok(swv)
    }

    fn write_word_32(&mut self, addr: u32, data: u32) -> Result<()> {
        self.op_start()?;
        self.sendcmd(&format!("mww 0x{:x} 0x{:x}", addr, data))?;
        self.op_done()?;
        Ok(())
    }

    fn write_8(&mut self, _addr: u32, _data: &[u8]) -> Result<()> {
        bail!("OpenOCD target does not support modifying state");
    }

    fn halt(&mut self) -> Result<()> {
        log::trace!("halting core");
        //
        // On OpenOCD, we don't halt. If GDB is connected, it gets really,
        // really confused!  This should probably be configurable at
        // some point...
        //
        // Well without unhalted read support cant do anything,
        // so we will pass the onus to the user for now
        self.sendcmd("halt")?;
        self.halted = true;
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        log::trace!("running core");
        //
        // Well, see above.
        //
        self.sendcmd("resume")?;
        self.halted = false;
        Ok(())
    }

    fn step(&mut self) -> Result<()> {
        todo!();
    }

    fn load(&mut self, path: &Path) -> Result<()> {
        self.sendcmd("reset init")?;
        self.sendcmd(&format!("load_image {} 0x0", path.display()))?;
        self.sendcmd(&format!("verify_image {} 0x0", path.display()))?;
        self.sendcmd("echo \"Doing reset\"")?;
        self.sendcmd("reset run")?;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.sendcmd("reset run")?;
        Ok(())
    }

    fn op_start(&mut self) -> Result<()> {
        log::trace!("op_start: halting core");
        self.halt()
    }

    fn op_done(&mut self) -> Result<()> {
        log::trace!("was halted: {}", self.was_halted);
        if !self.was_halted {
            log::trace!("op_done: resuming core");
            self.run()?;
        } else {
            log::trace!("op_done: leaving core halted");
        }
        Ok(())
    }
}
