use std::io;
use std::net::TcpStream;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

const WORKER_COUNT: usize = 32;
const REQUEST_QUEUE_CAPACITY: usize = 128;

pub fn serve_connections(
    incoming: impl Iterator<Item = io::Result<TcpStream>>,
    handler: impl Fn(TcpStream) + Send + Sync + 'static,
) {
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(REQUEST_QUEUE_CAPACITY);
    let receiver = Arc::new(Mutex::new(receiver));
    let handler = Arc::new(handler);
    let mut workers = Vec::with_capacity(WORKER_COUNT);

    for _ in 0..WORKER_COUNT {
        let receiver = Arc::clone(&receiver);
        let handler = Arc::clone(&handler);
        workers.push(thread::spawn(move || {
            loop {
                let stream = receiver.lock().expect("Request queue lock poisoned").recv();
                match stream {
                    Ok(stream) => handler(stream),
                    Err(_) => break,
                }
            }
        }));
    }

    for stream in incoming {
        match stream {
            Ok(stream) => {
                if sender.send(stream).is_err() {
                    eprintln!("Request workers have stopped");
                    break;
                }
            }
            Err(error) => eprintln!("Unable to connect: {error}"),
        }
    }

    drop(sender);
    for worker in workers {
        if let Err(error) = worker.join() {
            eprintln!("Request worker panicked: {error:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn blocked_request_does_not_serialize_other_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut first = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let first_stream = listener.accept().unwrap().0;
        let mut second = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let second_stream = listener.accept().unwrap().0;
        first.write_all(&[0]).unwrap();
        second.write_all(&[1]).unwrap();

        let (started_sender, started_receiver) = mpsc::channel();
        let (finished_sender, finished_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let release_receiver = Mutex::new(release_receiver);
        let server = thread::spawn(move || {
            serve_connections(
                vec![Ok(first_stream), Ok(second_stream)].into_iter(),
                move |mut stream| {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    if byte[0] == 0 {
                        started_sender.send(()).unwrap();
                        release_receiver.lock().unwrap().recv().unwrap();
                    } else {
                        finished_sender.send(()).unwrap();
                    }
                },
            );
        });

        let started = started_receiver.recv_timeout(Duration::from_secs(5));
        let finished = finished_receiver.recv_timeout(Duration::from_secs(5));
        release_sender.send(()).unwrap();
        server.join().unwrap();
        assert!(started.is_ok(), "First request should start");
        assert!(
            finished.is_ok(),
            "Second request should finish while the first request is blocked"
        );
    }
}
