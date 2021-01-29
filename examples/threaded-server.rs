use ark_std::rand::Rng;
use async_std::{io::BufWriter, net::TcpListener, prelude::*, task};
use bench_utils::*;
use clap::{App, Arg, ArgMatches};

extern crate io_utils;
use io_utils::{imux::IMuxAsync, threaded::ThreadedWriter};

fn get_random_buf(log_len: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 2usize.pow(log_len)];
    let mut rng = ark_std::test_rng();
    rng.fill(&mut buf[..]);
    buf
}

fn get_args() -> ArgMatches<'static> {
    App::new("threaded-server-example")
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
    let port = args.value_of("port").unwrap_or("8000");
    let server_addr = format!("0.0.0.0:{}", port);

    let num = if args.is_present("num") {
        clap::value_t!(args.value_of("num"), u32).unwrap()
    } else {
        25
    };
    let test_buf = get_random_buf(num);

    task::block_on(async move {
        // Form connections
        let listener = TcpListener::bind(server_addr).await.unwrap();
        let mut incoming = listener.incoming();
        let mut writers = Vec::with_capacity(16);
        for _ in 0..16 {
            let stream = incoming.next().await.unwrap().unwrap();
            writers.push(BufWriter::new(stream));
        }
        let writer = ThreadedWriter::new(IMuxAsync::new(writers));

        let send_time = start_timer!(|| "Spawning threads");
        crossbeam_utils::thread::scope(|s| {
            for i in 0..4 {
                let test_buf = &test_buf;
                let mut writer = writer.clone();
                s.spawn(move |_| {
                    task::block_on(async {
                        let send_time = start_timer!(|| format!("Thread {} Sending", i));
                        writer.write(&test_buf).await;
                        end_timer!(send_time);
                    });
                });
            }
        })
        .unwrap();
        end_timer!(send_time);
    });
}
