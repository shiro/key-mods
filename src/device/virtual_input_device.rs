use std::{fs, io, thread, time};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Result};
use evdev_rs::*;
use notify::{DebouncedEvent, Watcher};
use regex::Regex;
use tokio::sync::{mpsc, oneshot};
use tokio::task;
use walkdir::WalkDir;

use super::*;

fn get_fd_list(patterns: &Vec<Regex>) -> Vec<PathBuf> {
    let mut list = vec![];
    for entry in WalkDir::new("/dev/input")
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.file_type().is_file())
    {
        let name: String = String::from(entry.path().to_string_lossy());

        if !patterns.iter().any(|p| p.is_match(&name)) { continue; }
        list.push(PathBuf::from_str(&name).unwrap());
    }
    list
}


pub fn read_from_device_input_fd_thread_handler(
    device: Device,
    mut handler: impl FnMut(InputEvent),
    mut abort_rx: oneshot::Receiver<()>,
) {
    let mut a: io::Result<(ReadStatus, InputEvent)>;
    loop {
        if abort_rx.try_recv().is_ok() { return; }

        a = device.next_event(ReadFlag::NORMAL);
        if a.is_ok() {
            let mut result = a.ok().unwrap();
            match result.0 {
                ReadStatus::Sync => { // dropped, need to sync
                    while result.0 == ReadStatus::Sync {
                        a = device.next_event(ReadFlag::SYNC);
                        if a.is_ok() {
                            result = a.ok().unwrap();
                        } else { // something failed, abort sync and carry on
                            break;
                        }
                    }
                }
                ReadStatus::Success => {
                    handler(result.1);
                    // futures::executor::block_on(
                    //     reader_tx.send(result.1)
                    // ).unwrap();
                }
            }
        } else {
            let err = a.err().unwrap();
            match err.raw_os_error() {
                Some(libc::ENODEV) => { return; }
                Some(libc::EWOULDBLOCK) => {
                    // thread::yield_now();
                    thread::sleep(time::Duration::from_millis(2));
                    continue;
                }
                _ => {
                    println!("{:?}", err);
                    println!("reader loop err: {}", err);
                    return;
                }
            }
        }
    }
}


async fn runner_it(fd_path: &Path,
                   writer: mpsc::Sender<InputEvent>)
                   -> Result<oneshot::Sender<()>> {
    let fd_file = fs::OpenOptions::new()
        .read(true)
        .open(&fd_path)
        .expect(&*format!("failed to open fd '{}'", fd_path.to_str().unwrap_or("...")));

    let fd_file_nb = tokio_file_unix::File::new_nb(fd_file).unwrap();
    let mut device = Device::new_from_file(fd_file_nb).expect(&*format!("failed to open fd '{}'", fd_path.to_str().unwrap_or("...")));
    device.grab(GrabMode::Grab)
        .map_err(|err| anyhow!("failed to grab device '{}': {}", fd_path.to_string_lossy(), err))?;

    // spawn tasks for reading devices
    let (abort_tx, abort_rx) = oneshot::channel();
    thread::spawn(move || {
        read_from_device_input_fd_thread_handler(
            device,
            |ev| {
                let _ = futures::executor::block_on(
                    writer.send(ev)
                );
            },
            abort_rx,
        );
    });

    Ok(abort_tx)
}

async fn runner
(device_fd_path_pattens: Vec<Regex>,
 reader_init: oneshot::Sender<mpsc::Sender<InputEvent>>,
 writer: mpsc::Sender<InputEvent>,
) -> Result<()> {
    task::spawn(async move {
        let (fs_reader_tx, reader_rx) = mpsc::channel(128);

        // send the reader to the client
        reader_init.send(fs_reader_tx.clone()).unwrap();

        virtual_output_device::init_virtual_output_device(reader_rx).await
            .map_err(|err| anyhow!("uinput error: {}", err))
            .unwrap();

        #[derive(Debug)]
        enum FsWatchEvent {
            ADD(PathBuf),
            REMOVE(PathBuf),
        }

        let (fs_event_tx, mut fs_event_rx) = mpsc::channel(128);
        thread::spawn(move || -> Result<()> {
            let (watch_tx, watch_rx) = std::sync::mpsc::channel();
            let mut watcher: notify::RecommendedWatcher = notify::Watcher::new(watch_tx, time::Duration::from_secs(2))?;
            watcher.watch("/dev/input", notify::RecursiveMode::Recursive)?;

            loop {
                match watch_rx.recv() {
                    Ok(event) => {
                        use FsWatchEvent::*;
                        let fs_event = match event {
                            DebouncedEvent::Create(path_buf) => { ADD(path_buf) }
                            DebouncedEvent::Remove(path_buf) => { REMOVE(path_buf) }
                            _ => { continue; }
                        };

                        futures::executor::block_on(
                            fs_event_tx.send(fs_event)
                        ).unwrap();
                    }
                    Err(e) => return Err(anyhow!("watch error: {:?}", e)),
                }
            }
        });

        let mut device_map = HashMap::new();

        for device_fd_path in get_fd_list(&device_fd_path_pattens) {
            let res = runner_it(&device_fd_path, writer.clone()).await;
            let abort_tx = match res {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("{}", err);
                    continue;
                }
            };

            device_map.insert(device_fd_path, abort_tx);
        }

        loop {
            let fs_event = fs_event_rx.recv().await.unwrap();
            match fs_event {
                FsWatchEvent::ADD(path) => {
                    if !device_fd_path_pattens.iter().any(|regex| regex.is_match(path.to_str().unwrap())) {
                        continue;
                    }

                    let abort_tx = runner_it(&path, writer.clone()).await?;
                    device_map.insert(path, abort_tx);
                }
                FsWatchEvent::REMOVE(path) => {
                    if let Some(abort_tx) = device_map.remove(&path) {
                        // this might return an error if the device read thread crashed for any reason, ignore it since it was logged already
                        let _ = abort_tx.send(());
                    }
                }
            }
        }
        #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
    });

    Ok(())
}


pub async fn bind_udev_inputs(fd_patterns: &[impl AsRef<str>], reader_init_tx: oneshot::Sender<mpsc::Sender<InputEvent>>, writer_tx: mpsc::Sender<InputEvent>) -> Result<()> {
    let fd_patterns_regex = fd_patterns.into_iter()
        .map(|v| Regex::new(v.as_ref()))
        .collect::<std::result::Result<_, _>>()
        .map_err(|err| anyhow!("failed to parse regex: {}", err))?;

    task::spawn(async move {
        runner(fd_patterns_regex, reader_init_tx, writer_tx).await.unwrap();
        Ok::<(), anyhow::Error>(())
    });

    Ok(())
}
