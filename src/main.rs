use std::io;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

fn handle_write(mut stream: &TcpStream) -> io::Result<()> {
    let resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<html><body>OK</body></html>\r\n";
    stream.write(resp)?;
    Ok(())
}

fn handle_read(mut stream: &TcpStream) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(o) => {
                println!("{}", &String::from_utf8_lossy(&buf));
                if o < 4096 {
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
    let port = std::env::var("PORT").unwrap_or("8080".to_string());
    let listener = TcpListener::bind(format!("localhost:{port}"))?;
    println!("Listening for on port {}", port);

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
