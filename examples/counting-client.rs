use ark_std::rand::Rng;
use clap::{App, Arg, ArgMatches};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    net::TcpStream,
};

extern crate io_utils;
use io_utils::counting::CountingIO;

fn get_args() -> ArgMatches<'static> {
    App::new("counting-client-example")
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
        .get_matches()
}

fn send_and_receive<R: Read, W: Write>(reader: &mut R, writer: &mut W, to_send: &str) {
    let mut buf = [0u8; 1028];
    reader.read(&mut buf);

    let msg = String::from(to_send);
    writer.write(msg.as_bytes());
    writer.flush().unwrap();

    println!("Received message: \"{}\"", String::from_utf8_lossy(&buf));
    println!("Sent message: \"{}\"", msg);
}

fn main() {
    let args = get_args();
    let ip = args.value_of("ip").unwrap();
    let port = args.value_of("port").unwrap_or("8000");
    let server_addr = format!("{}:{}", ip, port);

    // Form connection
    let stream = TcpStream::connect(server_addr).unwrap();
    let mut reader = CountingIO::new(BufReader::new(&stream));
    let mut writer = CountingIO::new(BufWriter::new(&stream));

    send_and_receive(&mut reader, &mut writer, "Who's there?");
    println!(
        "Received {} bytes, sent {} bytes\n\n",
        reader.count(),
        writer.count()
    );

    // Reset counts
    reader.reset();
    writer.reset();

    send_and_receive(&mut reader, &mut writer, "Cargo who?");
    println!(
        "Received {} bytes, sent {} bytes",
        reader.count(),
        writer.count()
    );
}
