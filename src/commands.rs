// Copyright 2016 Mozilla Foundation
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use client::{connect_to_server,connect_with_retry,ServerConnection};
use cmdline::Command;
use protocol::{
    CacheStats,
    ClientRequest,
    GetStats,
    Shutdown,
};
use server;
use std::env;
use std::io::{self,Error,ErrorKind};
use std::process;

/// The default sccache server port.
pub const DEFAULT_PORT : u16 = 4225;

fn maybe_redirect_stdio(cmd : &mut process::Command) {
    if !match env::var("SCCACHE_NO_DAEMON") {
            Ok(val) => val == "1",
            Err(_) => false,
    } {
        cmd.stdin(process::Stdio::null())
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null());
    }
}

/// Re-execute the current executable as a background server.
fn run_server_process() -> io::Result<()> {
    env::current_exe().and_then(|exe_path| {
        let mut cmd = process::Command::new(exe_path);
        maybe_redirect_stdio(&mut cmd);
        cmd.env("SCCACHE_START_SERVER", "1")
            .spawn()
    }).and(Ok(()))
}

fn result_exit_code<T : FnOnce(io::Error)>(res : io::Result<()>,
                                           else_func : T) -> i32 {
    res.and(Ok(0)).unwrap_or_else(|e| {
        else_func(e);
        1
    })
}

fn connect_or_start_server(port : u16) -> io::Result<ServerConnection> {
    connect_to_server(port).or_else(|e| {
        if e.kind() == io::ErrorKind::ConnectionRefused {
            // If the connection was refused we probably need to start
            // the server.
            run_server_process().and_then(|()| connect_with_retry(port))
        } else {
            Err(e)
        }
    })
}

fn request_stats(mut conn : ServerConnection) -> io::Result<CacheStats> {
    println!("request_stats");
    let mut req = ClientRequest::new();
    req.set_get_stats(GetStats::new());
    //TODO: better error mapping
    let mut response = try!(conn.request(req).or(Err(Error::new(ErrorKind::Other, "Failed to send data to or receive data from server"))));
    if response.has_stats() {
        Ok(response.take_stats())
    } else {
        Err(Error::new(ErrorKind::Other, "Unexpected server response!"))
    }
}

fn request_shutdown(mut conn : ServerConnection) -> io::Result<CacheStats> {
    println!("request_shutdown");
    let mut req = ClientRequest::new();
    req.set_shutdown(Shutdown::new());
    //TODO: better error mapping
    let mut response = try!(conn.request(req).or(Err(Error::new(ErrorKind::Other, "Failed to send data to or receive data from server"))));
    if response.has_shuttingdown() {
        Ok(response.take_shuttingdown().take_stats())
    } else {
        Err(Error::new(ErrorKind::Other, "Unexpected server response!"))
    }
}

fn format_size(size : u64) -> String {
    //TODO: actual nice formatting
    format!("{} {}", size, "bytes")
}

fn print_stats(stats : CacheStats) -> io::Result<()> {
    for stat in stats.get_stats().iter() {
        //TODO: properly align output
        print!("{}\t\t", stat.get_name());
        if stat.has_count() {
            print!("{}", stat.get_count());
        } else if stat.has_str() {
            print!("{}", stat.get_str());
        } else if stat.has_size() {
            print!("{}", format_size(stat.get_size()));
        }
        print!("\n");
    }
    Ok(())
}

pub fn run_command(cmd : Command) -> i32 {
    match cmd {
        // Actual usage gets printed in `cmdline::parse`.
        Command::Usage => 1,
        Command::ShowStats => {
            result_exit_code(connect_or_start_server(DEFAULT_PORT).and_then(request_stats).and_then(print_stats),
                             |e| {
                                 println!("Couldn't get stats from server: {}", e);
                             })
        },
        Command::InternalStartServer => {
            // Can't report failure here, we're already daemonized.
            result_exit_code(server::start_server(DEFAULT_PORT), |_| {})
        },
        Command::StartServer => {
            println!("Starting sccache server...");
            result_exit_code(run_server_process(), |e| {
                println!("failed to spawn server: {}", e);
            })
        },
        Command::StopServer => {
            println!("Stopping sccache server...");
            result_exit_code(connect_to_server(DEFAULT_PORT).and_then(request_shutdown).and_then(print_stats),
                             |_e| {
                                 //TODO: check if this was connection refused,
                                 // print error if not.
                                 println!("Couldn't connect to server");
                             })

        },
        Command::Compile(compile_cmdline) => {
            result_exit_code(connect_or_start_server(DEFAULT_PORT).and_then(|_conn| {
                //TODO: send Compile request
                let cmd_str = compile_cmdline.join(" ");
                println!("Command: '{}'", cmd_str);
                Ok(())
            }),
                             |e| {
                                 println!("compile failed: {}", e);
                             })
        },
    }
}