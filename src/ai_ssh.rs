use crate::config::VpsEntry;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const PTY_READ_CHUNK_SIZE: usize = 16 * 1024;
const PTY_DRAIN_LIMIT_PER_TICK: usize = 128 * 1024;
#[cfg(windows)]
const SSH_COMMAND: &str = "ssh.exe";
#[cfg(not(windows))]
const SSH_COMMAND: &str = "ssh";

pub struct AiSshSession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    _reader_thread: thread::JoinHandle<()>,
}

impl AiSshSession {
    pub fn connect(entry: &VpsEntry) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 40,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new(SSH_COMMAND);
        command.arg("-tt");
        command.arg("-p");
        command.arg(entry.port.to_string());

        if let Some(ref key_path) = entry.key_path {
            command.arg("-i");
            command.arg(key_path);
        }

        command.arg(format!("{}@{}", entry.user, entry.host));

        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;
        let (output_tx, output_rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0_u8; PTY_READ_CHUNK_SIZE];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        if output_tx.send(buffer[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master: pair.master,
            child,
            writer,
            output_rx,
            _reader_thread: reader_thread,
        })
    }

    pub fn read_available(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let mut output = Vec::new();
        while output.len() < PTY_DRAIN_LIMIT_PER_TICK {
            match self.output_rx.try_recv() {
                Ok(chunk) => output.extend_from_slice(&chunk),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }

        Ok(String::from_utf8_lossy(&output).to_string())
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.writer.write_all(text.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn send_interrupt(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_text("\u{3}")
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), Box<dyn std::error::Error>> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn close(&mut self) {
        let _ = self.child.kill();
    }
}
