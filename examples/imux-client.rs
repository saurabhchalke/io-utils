use ark_std::rand::Rng;
use async_std::{io::BufReader, net::TcpStream, prelude::*, task};
use bench_utils::*;
use clap::{App, Arg, ArgMatches};

extern crate io_utils;
use io_utils::{counting::CountingIO, imux::IMuxAsync};

fn get_random_buf(log_len: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 2usize.pow(log_len)];
    let mut rng = ark_std::test_rng();
    rng.fill(&mut buf[..]);
    buf
}

fn get_args() -> ArgMatches<'static> {
    App::new("imux-client-example")
        .arg(
            Arg::with_name("ip")
                .short("i")
                .long("ip")
                .takes_value(true)
                .help("Server IP address")
                .required(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .takes_value(true)
                .help("Server port (default 8000)")
                .required(false),
        )
        .arg(
            Arg::with_name("num")
                .short("n")
                .long("num")
                .takes_value(true)
                .help("Log of the number of bytes to send (default 25)")
                .required(false),
        )
        .get_matches()
}

fn main() {
    let args = get_args();
    let ip = args.value_of("ip").unwrap();
    let port = args.value_of("port").unwrap_or("8000");
    let server_addr = format!("{}:{}", ip, port);

    let num = if args.is_present("num") {
        clap::value_t!(args.value_of("num"), u32).unwrap()
    } else {
        25
    };
    let test_buf = get_random_buf(num);

    task::block_on(async move {
        // Form connections
        let mut readers = Vec::with_capacity(16);
        for _ in 0..16 {
            let stream = TcpStream::connect(&server_addr).await.unwrap();
            readers.push(CountingIO::new(BufReader::new(stream)));
        }

        let read_time = start_timer!(|| "Reading buffer from 1 connection");
        // Read the message length
        let mut len_buf = [0u8; 8];
        readers[0].read_exact(&mut len_buf).await.unwrap();
        let len: u64 = u64::from_le_bytes(len_buf);
        // Read the rest of the message
        let mut buf = vec![0u8; len as usize];
        readers[0].read_exact(&mut buf[..]).await.unwrap();
        end_timer!(read_time);
        assert_eq!(buf, test_buf);

        add_to_trace!(|| "Communication", || format!(
            "Read {} bytes",
            readers[0].count()
        ));

        // Reset counts
        readers[0].reset();

        let read_time = start_timer!(|| "Reading buffer from 16 connections");
        let mut reader = IMuxAsync::new(readers);
        let result = reader.read().await.unwrap();
        end_timer!(read_time);
        assert_eq!(result, test_buf);

        add_to_trace!(|| "Communication", || format!(
            "Read {} bytes",
            reader.count()
        ));
    });
}
