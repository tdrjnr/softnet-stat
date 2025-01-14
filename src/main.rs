/*  Parser for /proc/softnet_stats file
 *  Copyright (C) 2016  Herman J. Radtke III <herman@hermanradtke.com>
 *
 *  This program is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
*/

use std::env;
use std::fs::File;
use std::io;

use getopts::Options;
use nom::character::complete::{char, line_ending};
use nom::combinator::{map, opt};
use nom::error::{Error, ErrorKind};
use nom::multi::many1;
use nom::number::complete::hex_u32;
use nom::sequence::{preceded, tuple};
use nom::{AsBytes, Err, IResult};
use serde_derive::{Deserialize, Serialize};

/// Network data processing statistics
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
struct SoftnetStat {
    /// The number of network frames processed.
    ///
    /// This can be more than the total number of network frames received if
    /// you are using ethernet bonding. There are cases where the ethernet
    /// bonding driver will trigger network data to be re-processed, which
    /// would increment the processed count more than once for the same packet.
    pub processed: u32,

    /// The number of network frames dropped because there was no room on the processing queue.
    pub dropped: u32,

    /// The number of times the `net_rx_action` loop terminated because the budget was consumed or
    /// the time limit was reached, but more work could have been.
    pub time_squeeze: u32,

    /// The number of times a collision occurred when trying to obtain a device lock
    /// when transmitting packets.
    ///
    /// This was removed in kernel v4.7
    pub cpu_collision: u32,

    /// The number of times this CPU has been woken up to process packets via an Inter-processor Interrupt.
    ///
    /// Support was added in kernel v2.6.36
    pub received_rps: Option<u32>,

    /// The number of times the flow limit has been reached.
    ///
    /// Flow limiting is an optional Receive Packet Steering feature.
    ///
    /// Support was added in kernel v3.11
    pub flow_limit_count: Option<u32>,

    /// The network backlog length.
    ///
    /// Support was added in kernel v5.10
    pub backlog_len: Option<u32>,

    /// The cpu_id is the CPU id owning this softnet data.
    ///
    /// There is not a direct match between softnet_stat
    /// lines and the related CPU. Offline CPUs are not dumped.
    ///
    /// Support was added in kernel v5.10
    pub cpu_id: Option<u32>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("j", "json", "use json output");
    opts.optflag("p", "prometheus", "use prometheus output");
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("s", "stdin", "read from stdin");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => panic!("Failed to parse options - {}", e),
    };

    if matches.opt_present("h") {
        print_usage(&program, opts);
        return;
    }

    let file = "/proc/net/softnet_stat";

    let raw = if matches.opt_present("s") {
        let handle = io::stdin();
        read_proc_file(handle).expect("Failed to read proc from stdin")
    } else {
        let handle = File::open(file).expect("Failed to open file");
        read_proc_file(handle).expect("Failed to read proc from file")
    };

    let stats = match parse_softnet_stats(&raw) {
        Ok((_, value)) => value,
        Err(Err::Incomplete(needed)) => {
            panic!("{} is in an unsupported format. Needed: {:?}", file, needed)
        }
        Err(Err::Error(e)) | Err(Err::Failure(e)) => {
            panic!("Error while parsing {}: {:?}", file, e)
        }
    };

    if matches.opt_present("j") {
        json(&stats);
    } else if matches.opt_present("p") {
        prometheus(&stats);
    } else {
        print(&stats, 15);
    }
}

fn read_proc_file<R>(mut handle: R) -> io::Result<Vec<u8>>
where
    R: io::Read,
{
    let mut buf = vec![];
    handle.read_to_end(&mut buf)?;

    Ok(buf)
}

fn parse_softnet_stats(input: &[u8]) -> IResult<&[u8], Vec<SoftnetStat>> {
    many1(parse_softnet_line)(input)
}

fn parse_softnet_line(input: &[u8]) -> IResult<&[u8], SoftnetStat> {
    if input.as_bytes().is_empty() {
        return Err(Err::Error(Error::new(input, ErrorKind::Eof)));
    }

    let line = tuple((
        hex_u32,                  // processed
        preceded(space, hex_u32), // dropped
        preceded(space, hex_u32), // time_squeeze
        preceded(space, hex_u32),
        preceded(space, hex_u32),
        preceded(space, hex_u32),
        preceded(space, hex_u32),
        preceded(space, hex_u32),
        preceded(space, hex_u32),      // cpu collision
        opt(preceded(space, hex_u32)), // received_rps
        opt(preceded(space, hex_u32)), // flow_limit_count
        opt(preceded(space, hex_u32)), // backlog_len
        opt(preceded(space, hex_u32)), // cpu_id
        line_ending,
    ));

    let mut parser = map(line, |result| SoftnetStat {
        processed: result.0,
        dropped: result.1,
        time_squeeze: result.2,
        cpu_collision: result.8,
        received_rps: result.9,
        flow_limit_count: result.10,
        backlog_len: result.11,
        cpu_id: result.12,
    });

    parser(input)
}

fn space(input: &[u8]) -> IResult<&[u8], char> {
    char(' ')(input)
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn print(stats: &[SoftnetStat], spacer: usize) {
    println!(
        "{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}",
        "Cpu",
        "Processed",
        "Dropped",
        "Time Squeezed",
        "Cpu Collision",
        "Received RPS",
        "Flow Limit Cnt",
        "Backlog Length",
        "CPU Id",
        spacer = spacer
    );

    for (i, stat) in stats.iter().enumerate() {
        println!(
            "{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}{:<spacer$}",
            i,
            stat.processed,
            stat.dropped,
            stat.time_squeeze,
            stat.cpu_collision,
            stat.received_rps.unwrap_or_default(),
            stat.flow_limit_count.unwrap_or_default(),
            stat.backlog_len.unwrap_or_default(),
            stat.cpu_id.unwrap_or_default(),
            spacer = spacer
        );
    }
}

fn json(stats: &[SoftnetStat]) {
    let data = serde_json::to_string(&stats).expect("Failed to encode stats into json format");
    println!("{}", data);
}

fn prometheus(stats: &[SoftnetStat]) {
    for (i, stat) in stats.iter().enumerate() {
        // Prior to Linux kernel v5.10, we used the index to determine the CPU Id. However, this is
        // not always correct as offline CPUs are not reported in the softnet data. If we are on a
        // Linux kernel that supports the cpu_id data, then we use that instead.
        let cpu_id = stat.cpu_id.unwrap_or(i as u32);

        println!(
            "softnet_frames_processed{{cpu=\"cpu{}\"}} {}",
            cpu_id, stat.processed
        );
        println!(
            "softnet_frames_dropped{{cpu=\"cpu{}\"}} {}",
            cpu_id, stat.dropped
        );
        println!(
            "softnet_time_squeeze{{cpu=\"cpu{}\"}} {}",
            cpu_id, stat.time_squeeze
        );
        println!(
            "softnet_cpu_collisions{{cpu=\"cpu{}\"}} {}",
            cpu_id, stat.cpu_collision
        );
        println!(
            "softnet_received_rps{{cpu=\"cpu{}\"}} {}",
            cpu_id,
            stat.received_rps.unwrap_or_default()
        );
        println!(
            "softnet_flow_limit_count{{cpu=\"cpu{}\"}} {}",
            cpu_id,
            stat.flow_limit_count.unwrap_or_default()
        );
        println!(
            "softnet_backlog_len{{cpu=\"cpu{}\"}} {}",
            cpu_id,
            stat.backlog_len.unwrap_or_default()
        );
    }
}

#[test]
fn test_parse_softnet_empty_line() {
    let raw = b"";

    // FIXME
    // Err(Err::Error((&raw[..] ErrorKind::Eof)))) should work, but there is some type inference
    // issue going on
    assert_eq!(parse_softnet_line(&raw[..]).is_err(), true,);
}

#[test]
fn test_parse_softnet_line() {
    let raw = b"6dcad223 00000000 00000001 00000000 00000000 00000000 00000000 00000000 00000000\n";

    let (remaining, value) = parse_softnet_line(&raw[..]).unwrap();

    assert_eq!(0, remaining.as_bytes().len());
    assert_eq!(
        SoftnetStat {
            processed: 1842008611,
            dropped: 0,
            time_squeeze: 1,
            cpu_collision: 0,
            received_rps: None,
            flow_limit_count: None,
            backlog_len: None,
            cpu_id: None,
        },
        value
    );
}

#[test]
fn test_parse_softnet_stats() {
    let pwd = env!("CARGO_MANIFEST_DIR");
    let files = vec![
        format!("{}/tests/proc-net-softnet_stat-2_6_32", pwd),
        format!("{}/tests/proc-net-softnet_stat-2_6_36", pwd),
        format!("{}/tests/proc-net-softnet_stat-3_11", pwd),
        format!("{}/tests/proc-net-softnet_stat-5_10_47", pwd),
    ];

    for file in files.iter() {
        let handle = File::open(file).unwrap();
        let raw = read_proc_file(handle).unwrap();

        let _ = parse_softnet_stats(&raw).unwrap();
    }
}
