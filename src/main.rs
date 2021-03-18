extern crate epoll;
extern crate log;
extern crate vhost;
extern crate vhost_user_backend;
extern crate vm_memory;

use std::sync::{Arc, Mutex, RwLock};
use std::{convert, error, fmt, io, process};

use clap::{crate_authors, crate_version, App, Arg};
use libc::EFD_NONBLOCK;
use log::*;
use vhost::vhost_user::message::*;
use vhost::vhost_user::Listener;
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon, Vring};
use virtio_bindings::bindings::virtio_net::*;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};
use vmm_sys_util::eventfd::EventFd;

const QUEUE_SIZE: usize = 1024;
// The guest queued an available buffer for the request queue.
const REQ_QUEUE_EVENT: u16 = 1;

type Result<T> = std::result::Result<T, Error>;
type VhostUserBackendResult<T> = std::result::Result<T, std::io::Error>;

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
        write!(f, "vhost_user_block_error: {:?}", self)
    }
}

impl error::Error for Error {}

impl convert::From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

struct VhostUserInputThread {
    // evdevfd: int,
    kill_evt: EventFd,
}

impl VhostUserInputThread {
    fn new() -> Result<Self> {
        Ok(VhostUserInputThread {
            kill_evt: EventFd::new(EFD_NONBLOCK).map_err(Error::CreateKillEventFd)?,
        })
    }

    fn process_queue(&mut self, vring: &mut Vring) -> bool {
        true
    }
}

struct VhostUserInputBackend {
    thread: Mutex<VhostUserInputThread>,
}

impl VhostUserInputBackend {
    fn new() -> Result<Self> {
        let thread = Mutex::new(VhostUserInputThread::new()?);
        Ok(VhostUserInputBackend { thread })
    }
}

impl VhostUserBackend for VhostUserInputBackend {
    fn num_queues(&self) -> usize {
        1
    }

    fn max_queue_size(&self) -> usize {
        QUEUE_SIZE
    }

    fn features(&self) -> u64 {
        1 << VIRTIO_F_VERSION_1 | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::STATUS | VhostUserProtocolFeatures::MQ
    }

    fn set_event_idx(&mut self, enabled: bool) {}

    fn update_memory(
        &mut self,
        atomic_mem: GuestMemoryAtomic<GuestMemoryMmap>,
    ) -> VhostUserBackendResult<()> {
        unimplemented!()
    }

    fn handle_event(
        &self,
        device_event: u16,
        evset: epoll::Events,
        vrings: &[Arc<RwLock<Vring>>],
        thread_id: usize,
    ) -> VhostUserBackendResult<bool> {
        unimplemented!()
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

    let input_backend = Arc::new(RwLock::new(VhostUserInputBackend::new().unwrap()));
    println!("VhostUserInputBackend created...");
    let mut daemon =
        VhostUserDaemon::new("vhost-user-input".to_string(), input_backend.clone()).unwrap();
    println!("VhostUserDaemon created...");

    if let Err(e) = daemon.start(listener) {
        error!("Failed to start daemon: {:?}", e);
        process::exit(1);
    }

    if let Err(e) = daemon.wait() {
        error!("Waiting for daemon failed: {:?}", e);
    }

    let kill_evt = input_backend
        .read()
        .unwrap()
        .thread
        .lock()
        .unwrap()
        .kill_evt
        .try_clone()
        .unwrap();
    if let Err(e) = kill_evt.write(1) {
        error!("Error shutting down worker thread: {:?}", e)
    }
}
