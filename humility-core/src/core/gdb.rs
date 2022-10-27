// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, bail, ensure, Result};

use crate::regs::Register;
use roxmltree::Document;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::str;
use std::time::Duration;
use xmlparser::{Token, Tokenizer};

use crate::core::Core;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum GDBServer {
    OpenOCD,
    JLink,
    Qemu,
}

impl fmt::Display for GDBServer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                GDBServer::OpenOCD => "OpenOCD",
                GDBServer::JLink => "JLink",
                GDBServer::Qemu => "QEMU",
            }
        )
    }
}

pub struct GDBCore {
    stream: TcpStream,
    server: GDBServer,
    halted: bool,
    was_halted: bool,
    reg_table: HashMap<String, u32>,
}

const GDB_PACKET_START: char = '$';
const GDB_PACKET_END: char = '#';
const GDB_PACKET_ACK: char = '+';

#[rustfmt::skip::macros(anyhow, bail)]
impl GDBCore {
    fn prepcmd(&mut self, cmd: &str) -> Vec<u8> {
        let mut payload = vec![GDB_PACKET_START as u8];

        let mut cksum = 0;

        for b in cmd.as_bytes() {
            payload.push(*b);
            cksum += *b as u32;
        }

        //
        // Tack on the goofy checksum beyond the end of the packet.
        //
        let trailer = &format!("{}{:02x}", GDB_PACKET_END, cksum % 256);

        for b in trailer.as_bytes() {
            payload.push(*b);
        }

        log::trace!("sending {}", str::from_utf8(&payload).unwrap());
        payload
    }

    fn firecmd(&mut self, cmd: &str) -> Result<()> {
        log::trace!("sending: {}", cmd);
        let payload = self.prepcmd(cmd);
        self.stream.write_all(&payload)?;
        log::trace!("sent");
        Ok(())
    }

    // GDB support is still WIP, so may need later
    #[allow(unused)]
    fn recvack(&mut self) -> Result<()> {
        let mut rbuf = vec![0; 1];

        let rval = self.stream.read(&mut rbuf)?;
        // should get ACK, aka '+' or 0x2b
        // ensure we got our 1 byte
        ensure!(rval == 1);
        // ensure that byte is the the ack
        ensure!(rbuf[0] == GDB_PACKET_ACK as u8);
        log::trace!("received ack");
        Ok(())
    }

    fn sendack(&mut self) -> Result<()> {
        self.stream.write_all(&[GDB_PACKET_ACK as u8])?;
        log::trace!("sending ack");
        Ok(())
    }

    fn recvdata(&mut self) -> Result<String> {
        let mut rbuf = vec![0; 1024];
        let mut result = String::new();

        log::trace!("reading first chunk");
        loop {
            let rval = self.stream.read(&mut rbuf)?;
            log::trace!("received {} bytes", rval);
            result.push_str(str::from_utf8(&rbuf[0..rval])?);
            log::trace!("response: {}", result);

            //
            // We are done when we have our closing delimter followed by
            // the two byte checksum.
            //
            let end_yet = result.find(GDB_PACKET_END);
            if end_yet.is_none() {
                log::trace!("reading more data");
                continue;
            }
            if result.find(GDB_PACKET_END) == Some(result.len() - 3) {
                break;
            }
            log::trace!("reading more data");
        }

        //
        // In our result, we should have exactly one opening and exactly
        // one closing delimiter -- and, if expectack is set, at least
        // one ACK as well.
        //
        let start = match result.find(GDB_PACKET_START) {
            Some(ndx) => ndx,
            None => {
                bail!("missing start of packet: \"{}\"", result);
            }
        };

        //
        // By merits of being here, we know we have our end-of-packet...
        //
        let end = result.find(GDB_PACKET_END).unwrap();

        if end < start {
            bail!("start/end inverted: \"{}\"", result);
        }

        Ok(result[start + 1..end].to_string())
    }

    fn sendcmd(&mut self, cmd: &str) -> Result<String> {
        let mut just_halted = false;
        self.firecmd(cmd)?;
        self.recvack()?;

        let mut data = self.recvdata()?;
        // if core halted
        if data.contains("T02thread") {
            self.halted = true;
            just_halted = true;
            self.sendack()?;
            log::trace!("halted: trying again");
            self.firecmd(cmd)?;
            data = self.recvdata()?;
        }
        if just_halted {
            self.firecmd("c")?;
            self.halted = false;
        }
        if data.len() == 3 && data.starts_with('E') {
            bail!("received error code: {}", data)
        } else {
            Ok(data)
        }
    }

    pub fn new(server: GDBServer) -> Result<GDBCore> {
        let port = match server {
            GDBServer::OpenOCD => 3333,
            GDBServer::JLink => 2331,
            GDBServer::Qemu => 3333,
        };

        let host = format!("127.0.0.1:{}", port);
        let addr = host.parse()?;
        let timeout = Duration::from_millis(100);

        let stream =
            TcpStream::connect_timeout(&addr, timeout).map_err(|_| {
                anyhow!(
                "can't connect to {} GDB server on \
                    port {}; is it running?",
                server, port
            )
            })?;

        // set read timout to avoid blocking when waiting for a response that never comes.  This
        // allows an explicit error
        stream.set_read_timeout(Some(Duration::from_millis(1000)))?;
        stream.set_write_timeout(Some(Duration::from_millis(1000)))?;

        //
        // Both the OpenOCD and JLink GDB servers stop the target upon
        // connection.  This is helpful in that we know the state that
        // we're in -- but it's also not the state that we want to be
        // in.  We explicitly run the target before returning.
        //
        let mut core = Self {
            stream,
            server,
            halted: true,
            was_halted: true,
            reg_table: HashMap::new(),
        };

        let data = core.recvdata();
        match data {
            Err(_err) => {
                log::trace!("connected to halted core");
                core.was_halted = true;
            }
            Ok(data) => {
                // When gdb halts the core, it sends this packet back.
                // [Here](https://sourceware.org/gdb/onlinedocs/gdb/Stop-Reply-Packets.html#Stop-Reply-Packets) is the reference for decoding.
                // It is decoded to mean that thread 1 halted.
                // It is used here to determine if the core was halted when humility connected, as any connection to the gdb server halts the core.
                // If the core was already halted, this packet will not be received.
                if !data.contains("T02thread") {
                    bail!("Target did not halt on connect");
                }
                log::trace!("connected to running core");
                core.was_halted = false;
                core.run()?;
            }
        };

        let supported = core.sendcmd("qSupported")?;
        log::trace!("{} supported string: {}", server, supported);
        // need to call to enable single register reads
        // see: https://github.com/qemu/qemu/blob/e750a7ace492f0b450653d4ad368a77d6f660fb8/gdbstub/gdbstub.c#L1600
        let feature_read =
            core.sendcmd("qXfer:features:read:target.xml:0,ffb")?;
        let feature_read = &mut feature_read.chars();
        feature_read.next();
        log::trace!("feature read string: {:?}", feature_read);
        core.feature_xml_parser(feature_read.as_str());
        log::trace!("reg table: {:?}", core.reg_table);
        Ok(core)
    }

    // TODO
    // The parsing assumes an precise xml structure that might not be true if the gdbstub changes.
    // It also only parses for the `regnum` attribute.
    // We have to use the `xmlparser` crate here as `roxmltree` will attempt to parse the includes
    // when we don't actually have them yet...
    fn feature_xml_parser(&mut self, xml_string: &str) {
        let tokens = Tokenizer::from(xml_string);

        // Each include will be an attribute token
        let includes = tokens.filter_map(|token| match token {
            Ok(Token::Attribute { local, value, .. }) => {
                //TODO we are assuming the only hrefs within the xml are for includes...
                // `local` is the name of the attribute
                if "href" == local.as_str() {
                    Some(value.as_str())
                } else {
                    None
                }
            }
            _ => None,
        });

        // Request and parse out the register numbers from each included file
        for include in includes {
            self.request_and_parse(include);
        }
    }

    fn fetch_xml(&mut self, xml_file: &str) -> Result<String> {
        // request the xml
        // we unwrap since the server told us that the xml exist, so we should receive it,
        // otherwise something is broken
        //
        let mut len_read = 0;
        let mut features = "".to_owned();
        loop {
            let data = self.sendcmd(
                format!("qXfer:features:read:{}:{:x},ffb", xml_file, len_read)
                    .as_str(),
            )?;
            len_read += data.len() - 1;

            let mut data = data.chars();
            // the first char will be 'l' or 'm' to indicate if more xml data is avaliable
            let first_char = data.next().unwrap();

            features.push_str(data.as_str());
            if first_char == 'l' {
                break;
            }
        }

        Ok(features)
    }

    // This function uses the higher level `roxmltree` crate as it is easier to use
    fn request_and_parse(&mut self, xml_file: &str) {
        log::trace!("parsing include: {}", xml_file);
        // request the included xml
        // we unwrap since the server told us that the xml exist, so we should receive it,
        // otherwise something is broken
        let features = self.fetch_xml(xml_file).unwrap();
        log::trace!("whole xml: {}", features);
        let doc = Document::parse(features.as_str()).unwrap();
        for feature in doc.root_element().children() {
            // only parse the reg tags for now
            if feature.tag_name().name() == "reg" {
                let attributes = feature.attributes();
                if attributes.len() != 3 {
                    continue;
                }
                // TODO
                // the attribute locations are hardcoded for now, otherwise I would have to scan
                // them all multiple times
                let name = attributes[0].value();
                if attributes[2].name() != "regnum" {
                    continue;
                }
                let regnum: u32 = attributes[2].value().parse().unwrap();
                self.reg_table.insert(name.to_owned(), regnum);
            }
        }
    }
}

#[rustfmt::skip::macros(anyhow, bail)]
impl Core for GDBCore {
    fn info(&self) -> (String, Option<String>) {
        ("GDB".to_string(), None)
    }

    fn read_word_32(&mut self, addr: u32) -> Result<u32> {
        let mut data = [0; 4];
        self.read_8(addr, &mut data)?;
        Ok(u32::from_le_bytes(data))
    }

    fn read_8(&mut self, addr: u32, data: &mut [u8]) -> Result<()> {
        let cmd = format!("m{:x},{:x}", addr, data.len());

        let rstr = self.sendcmd(&cmd)?;

        if rstr.len() > data.len() * 2 {
            bail!("bad read_8 on cmd {} \
                (expected {}, found {}): {}",
                cmd, data.len() * 2, rstr.len(), rstr);
        }

        for (idx, i) in (0..rstr.len()).step_by(2).enumerate() {
            data[idx] = u8::from_str_radix(&rstr[i..=i + 1], 16)?;
        }

        Ok(())
    }

    fn read_reg(&mut self, reg: Register) -> Result<u64> {
        log::trace!("reading reg: {:?}", reg);
        let reg_id = if self.reg_table.is_empty()
            || reg.is_general_purpose()
            || reg.is_pc()
        {
            reg.to_gdb_id()
        } else {
            let reg_string = reg.to_string().to_lowercase();
            log::trace!("checking for reg: {}", reg_string);
            if let Some(id) = self.reg_table.get(&reg_string) {
                *id
            } else {
                bail!(
                    "register table provided, but does not contains: {}",
                    reg_string
                );
            }
        };

        let cmd = &format!("p{:02X}", reg_id);

        let rstr = self.sendcmd(cmd)?;
        let mut buf = vec![];

        // now we have to decode the register value
        for i in (0..rstr.len()).step_by(2) {
            buf.push(u8::from_str_radix(&rstr[i..=i + 1], 16)?);
        }
        match rstr.len() {
            8 => Ok(u32::from_le_bytes(buf[..].try_into().unwrap()) as u64),
            16 => Ok(u64::from_le_bytes(buf[..].try_into().unwrap())),
            _ => bail!("invalid register response"),
        }
    }

    fn write_reg(&mut self, _reg: Register, _value: u64) -> Result<()> {
        Err(anyhow!(
            "{} GDB target does not support modifying state", self.server
        ))
    }

    fn write_word_32(&mut self, _addr: u32, _data: u32) -> Result<()> {
        Err(anyhow!(
            "{} GDB target does not support modifying state", self.server
        ))
    }

    fn write_8(&mut self, _addr: u32, _data: &[u8]) -> Result<()> {
        Err(anyhow!(
            "{} GDB target does not support modifying state", self.server
        ))
    }

    fn halt(&mut self) -> Result<()> {
        //target is halted whenever a command is sent, so just send help
        log::trace!("halting");
        self.firecmd("h")?;
        let reply = self.recvdata()?;
        log::trace!("halt reply: {}", reply);
        self.halted = true;
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        //
        // The OpenOCD target in particular loses its mind if told to
        // continue to when it's already running, insisting on
        // sending a reply with an elaborate message that we don't
        // know to wait on -- so we only continue a target if we know
        // it to be halted.
        //
        if self.halted {
            log::trace!("running core");
            self.firecmd("c")?;
            self.halted = false;
        }

        Ok(())
    }

    fn step(&mut self) -> Result<()> {
        Ok(())
    }

    fn init_swv(&mut self) -> Result<()> {
        Ok(())
    }

    fn read_swv(&mut self) -> Result<Vec<u8>> {
        Err(anyhow!("GDB target does not support SWV"))
    }

    fn load(&mut self, _path: &Path) -> Result<()> {
        bail!("Flash loading is not supported with GDB");
    }

    fn reset(&mut self) -> Result<()> {
        bail!("Reset is not supported with GDB");
    }
}
