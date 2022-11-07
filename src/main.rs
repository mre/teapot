use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

const READ_BUF_SIZE: usize = 512;

const TEAPOT_RESPONSE: &[u8; 64] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nOK\r\n";

fn handle_write(mut stream: &TcpStream) -> io::Result<()> {
    stream.write_all(TEAPOT_RESPONSE)?;
    Ok(())
}

fn handle_read(mut stream: &TcpStream) -> io::Result<()> {
    let mut buf = [0u8; READ_BUF_SIZE];
    loop {
        match stream.read(&mut buf) {
            Ok(bytes_read) => {
                println!("{}", &String::from_utf8_lossy(&buf));
                if bytes_read < READ_BUF_SIZE {
                    break;
                }
            }
            Err(e) => {
                println!("Unable to read stream: {}", e);
                stream.shutdown(Shutdown::Both)?;
                return Err(e);
            }
        }
    }
    Ok(())
}

fn handle_client(stream: TcpStream) -> io::Result<()> {
    loop {
        handle_read(&stream)?;
        handle_write(&stream)?;
        stream.shutdown(Shutdown::Both)?;
    }
}
fn main() -> io::Result<()> {
    // Load port from env; default to 8080
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let listener = TcpListener::bind(format!("localhost:{port}"))?;
    println!("🫖 teapot listening for on port {}...", port);

    for stream in listener.incoming() {
        match stream {
            Ok(conn) => {
                thread::spawn(|| handle_client(conn));
            }
            Err(e) => {
                println!("Unable to connect:{}", e);
            }
        }
    }
    Ok(())
}
