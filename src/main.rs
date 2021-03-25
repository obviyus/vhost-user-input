extern crate epoll;
extern crate log;
extern crate vhost;
extern crate vhost_user_backend;
extern crate vm_memory;

use std::{convert, error, fmt, io, process, result};
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex, RwLock};

use clap::{App, Arg, crate_authors, crate_version};
use libc::EFD_NONBLOCK;
use log::*;
use vhost::vhost_user::Listener;
use vhost::vhost_user::message::*;
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon, Vring, VringWorker};
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
    input_fd: EventFd,
    vring_worker: Option<Arc<VringWorker>>,
    event_idx: bool,
    kill_evt: EventFd,
}

impl VhostUserInputThread {
    // Create a new virtio input device
    fn new(input_fd: EventFd) -> Result<Self> {
        println!("new VhostUserInputThread");

        Ok(VhostUserInputThread {
            input_fd,
            vring_worker: None,
            event_idx: false,
            kill_evt: EventFd::new(EFD_NONBLOCK).map_err(Error::CreateKillEventFd)?,
        })
    }

    fn process_queue(&mut self, vring: &mut Vring) -> bool {
        let mut used_any: bool = false;
        while let Some(_) = vring.mut_queue().iter().unwrap().next() {
            println!("got an element in the queue!");
            used_any = true;
        }

        used_any
    }

    fn set_vring_worker(&mut self, vring_worker: Option<Arc<VringWorker>>) {
        self.vring_worker = vring_worker;
    }
}

struct VhostUserInputBackend {
    threads: Vec<Mutex<VhostUserInputThread>>,
    queues_per_thread: Vec<u64>,
    queue_size: usize,
}

impl VhostUserInputBackend {
    fn new(input_fd: EventFd, num_queues: usize, queue_size: usize) -> Result<Self> {
        let mut queues_per_thread = Vec::new();
        let mut threads = Vec::new();

        for i in 0..num_queues {
            let thread = Mutex::new(VhostUserInputThread::new(input_fd.try_clone().unwrap())?);
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
        println!("num_queues");

        QUEUE_SIZE
    }

    fn max_queue_size(&self) -> usize {
        println!("max_queue_size");

        self.queue_size as usize
    }

    fn features(&self) -> u64 {
        println!("features");

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

    fn queues_per_thread(&self) -> Vec<u64> {
        self.queues_per_thread.clone()
    }
}

fn main() {
    // CLI args needed for a complete vhost-user-input implementation
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

    // Socket on which the vhost-user-input server listens on
    let socket_path = match cmd_arguments.value_of("socket-path") {
        None => {
            panic!("no socket-path provided!")
        }
        Some(path) => path,
    };

    // Add a new listener on the socket-path to listen for events
    let listener = Listener::new(socket_path, true).unwrap();
    // TODO: Implement logging
    println!("listening on {}", socket_path);

    // EventFd for synthetic inputs to the VhostUserInputThread
    let sim_inputs = EventFd::new(EFD_NONBLOCK).unwrap();

    let input_backend = Arc::new(RwLock::new(VhostUserInputBackend::new(sim_inputs.try_clone().unwrap(), 2, 1024).unwrap()));
    println!("VhostUserInputBackend created...");

    let mut daemon =
        VhostUserDaemon::new("vhost-user-input".to_string(), input_backend.clone()).unwrap();
    println!("VhostUserDaemon created...");

    if let Err(e) = daemon.start(listener) {
        error!("Failed to start daemon: {:?}", e);
        process::exit(1);
    }
    println!("VhostUserDaemon started...");

    // Get vring_workers from the VhostUserInputThread, register listeners on each of them for
    // synthetic inputs EventFd created earlier
    let vring_workers = daemon.get_vring_workers();
    for vring_worker in vring_workers {
        // Send dummy data for now
        if let Err(e) = vring_worker.register_listener(sim_inputs.as_raw_fd(), epoll::Events::EPOLLIN, 0) {
            error!("Failed to register VringWorker: {:?}", e);
            process::exit(1)
        }
    }

    if let Err(e) = daemon.wait() {
        error!("Waiting for daemon failed: {:?}", e);
    }

    println!("Waiting complete");

    for thread in input_backend.read().unwrap().threads.iter() {
        let kill_evt = thread
            .lock()
            .unwrap()
            .kill_evt
            .try_clone()
            .unwrap();
        if let Err(e) = kill_evt.write(1) {
            error!("Error shutting down worker thread: {:?}", e)
        }
    }
}
