extern crate epoll;
extern crate log;
extern crate vhost;
extern crate vhost_user_backend;
extern crate vm_memory;

use std::sync::{Arc, Mutex, RwLock};
use std::{convert, error, fmt, io, process, result};

use clap::{crate_authors, crate_version, App, Arg};
use libc::EFD_NONBLOCK;
use log::*;
use vhost::vhost_user::message::*;
use vhost::vhost_user::Listener;
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon, Vring};
use virtio_bindings::bindings::virtio_blk::VIRTIO_F_VERSION_1;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};
use vmm_sys_util::eventfd::EventFd;

type Result<T> = std::result::Result<T, Error>;
type VhostUserBackendResult<T> = std::result::Result<T, std::io::Error>;

const QUEUE_SIZE: usize = 1;

#[derive(Debug)]
enum Error {
    /// Failed to create kill eventfd
    CreateKillEventFd(io::Error),
    /// Failed to handle event other than input event.
    HandleEventNotEpollIn,
    /// Failed to handle unknown event.
    HandleEventUnknownEvent,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vhost_user_input_error: {:?}", self)
    }
}

impl error::Error for Error {}

impl convert::From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

struct VhostUserInputThread {
    mem: Option<GuestMemoryAtomic<GuestMemoryMmap>>,
    event_idx: bool,
    kill_evt: EventFd,
}

impl VhostUserInputThread {
    // Create a new virtio input device
    fn new() -> Result<Self> {
        Ok(VhostUserInputThread {
            mem: None,
            event_idx: false,
            kill_evt: EventFd::new(EFD_NONBLOCK).map_err(Error::CreateKillEventFd)?,
        })
    }

    fn process_queue(&mut self, vring: &mut Vring) -> bool {
        let mut used_any: bool = false;
        // let mem = match self.mem.as_ref() {
        //     Some(m) => m,
        //     None => return false,
        // };

        while let Some(_) = vring.mut_queue().iter().unwrap().next() {
            println!("got an element in the queue!");
            used_any = true;
        }

        used_any
    }
}

struct VhostUserInputBackend {
    threads: Vec<Mutex<VhostUserInputThread>>,
    queues_per_thread: Vec<u64>,
    queue_size: usize,
}

impl VhostUserInputBackend {
    fn new(num_queues: usize, queue_size: usize) -> Result<Self> {
        let mut queues_per_thread = Vec::new();
        let mut threads = Vec::new();

        for i in 0..num_queues {
            let thread = Mutex::new(VhostUserInputThread::new()?);
            threads.push(thread);
            queues_per_thread.push(0b1 << i);
        }

        Ok(VhostUserInputBackend {
            threads,
            queues_per_thread,
            queue_size,
        })
    }
}

impl VhostUserBackend for VhostUserInputBackend {
    fn num_queues(&self) -> usize {
        QUEUE_SIZE
    }

    fn max_queue_size(&self) -> usize {
        self.queue_size as usize
    }

    fn features(&self) -> u64 {
        1 << VIRTIO_F_VERSION_1 | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::STATUS | VhostUserProtocolFeatures::MQ
    }

    fn set_event_idx(&mut self, enabled: bool) {
        for thread in self.threads.iter() {
            thread.lock().unwrap().event_idx = enabled;
        }
    }

    fn update_memory(
        &mut self,
        mem: GuestMemoryAtomic<GuestMemoryMmap>,
    ) -> VhostUserBackendResult<()> {
        for thread in self.threads.iter() {
            thread.lock().unwrap().mem = Some(mem.clone());
        }
        Ok(())
    }

    fn handle_event(
        &self,
        device_event: u16,
        evset: epoll::Events,
        vrings: &[Arc<RwLock<Vring>>],
        thread_id: usize,
    ) -> VhostUserBackendResult<bool> {
        if evset != epoll::Events::EPOLLIN {
            return Err(Error::HandleEventNotEpollIn.into());
        }
        println!("event received: {:#?}", device_event);

        Ok(false)
    }

    fn get_config(&self, _offset: u32, _size: u32) -> Vec<u8> {
        println!("get_config() called");
        Vec::new()
    }

    fn set_config(&mut self, _offset: u32, _buf: &[u8]) -> result::Result<(), io::Error> {
        println!("sett_config() called");
        Ok(())
    }

    // fn exit_event(&self, thread_index: usize) -> Option<(EventFd, Option<u16>)> {
    //     // The exit event is placed after the queue, which is event index 1.
    //     Some((
    //         self.threads[thread_index]
    //             .lock()
    //             .unwrap()
    //             .kill_evt
    //             .try_clone().unwrap(),
    //         Some(1),
    //     ))
    // }

    fn queues_per_thread(&self) -> Vec<u64> {
        self.queues_per_thread.clone()
    }
}

fn main() {
    let cmd_arguments = App::new("vhost user input")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Run vhost-user-input")
        .arg(
            Arg::with_name("print-capabilities")
                .long("print-capabilities")
                .help("Print capabilities")
                .takes_value(true)
                .min_values(1),
        )
        .arg(
            Arg::with_name("no-grab")
                .long("no-grab")
                .help("Don't grab device")
                .takes_value(true)
                .min_values(1),
        )
        .arg(
            Arg::with_name("socket-path")
                .long("socket-path")
                .help("vhost-user socket path")
                .takes_value(true)
                .min_values(1)
                .required(true),
        )
        .arg(
            Arg::with_name("fd")
                .long("fd")
                .help("Use inherited fd socket")
                .takes_value(true)
                .min_values(1),
        )
        .arg(
            Arg::with_name("evdev-path")
                .long("evdev-path")
                .help("evdev input device path")
                .takes_value(true)
                .min_values(1),
        )
        .get_matches();

    let socket_path = match cmd_arguments.value_of("socket-path") {
        None => {
            panic!("no socket-path provided!")
        }
        Some(path) => path,
    };

    let listener = Listener::new(socket_path, true).unwrap();
    println!("listening on {}", socket_path);

    let input_backend = Arc::new(RwLock::new(VhostUserInputBackend::new(2, 1024).unwrap()));
    println!("VhostUserInputBackend created...");

    let mut daemon =
        VhostUserDaemon::new("vhost-user-input".to_string(), input_backend.clone()).unwrap();
    println!("VhostUserDaemon created...");

    if let Err(e) = daemon.start(listener) {
        error!("Failed to start daemon: {:?}", e);
        process::exit(1);
    }
    println!("VhostUserDaemon started...");

    if let Err(e) = daemon.wait() {
        error!("Waiting for daemon failed: {:?}", e);
    }

    // let vring_workers = daemon.get_vring_workers();

    for thread in input_backend.read().unwrap().threads.iter() {
        if let Err(e) = thread.lock().unwrap().kill_evt.write(1) {
            error!("Error shutting down worker thread: {:?}", e)
        }
    }
}
