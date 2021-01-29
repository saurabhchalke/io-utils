use ark_std::rand::Rng;
use async_std::{io::BufReader, net::TcpStream, task};
use bench_utils::*;
use clap::{App, Arg, ArgMatches};

extern crate io_utils;
use io_utils::{imux::IMuxAsync, threaded::ThreadedReader};

fn get_random_buf(log_len: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 2usize.pow(log_len)];
    let mut rng = ark_std::test_rng();
    rng.fill(&mut buf[..]);
    buf
}

fn get_args() -> ArgMatches<'static> {
    App::new("threaded-client-example")
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
            readers.push(BufReader::new(stream));
        }
        let reader = ThreadedReader::new(IMuxAsync::new(readers));

        let recv_time = start_timer!(|| "Spawning threads");
        crossbeam_utils::thread::scope(|s| {
            for i in 0..4 {
                let test_buf = &test_buf;
                let mut reader = reader.clone();
                s.spawn(move |_| {
                    task::block_on(async {
                        let recv_time = start_timer!(|| format!("Thread {} Receiving", i));
                        let buf = reader.read().await;
                        end_timer!(recv_time);
                        assert_eq!(test_buf, buf.as_slice());
                    });
                });
            }
        })
        .unwrap();
        end_timer!(recv_time);
    });
}
