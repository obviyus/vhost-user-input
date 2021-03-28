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
use vhost_user_backend::{VhostUserBackend, VhostUserDaemon, Vring, VringWorker};
use virtio_bindings::bindings::virtio_blk::VIRTIO_F_VERSION_1;
use vm_memory::{GuestMemoryAtomic, GuestMemoryMmap};
use vmm_sys_util::eventfd::EventFd;

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
        write!(f, "vhost_user_input_error: {:?}", self)
    }
}

impl error::Error for Error {}

impl convert::From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

// const VIRTIO_INPUT_CFG_UNSET: u32 = 0x00;
const VIRTIO_INPUT_CFG_ID_NAME: u32 = 0x01;
const VIRTIO_INPUT_CFG_ID_SERIAL: u32 = 0x02;
const VIRTIO_INPUT_CFG_ID_DEVIDS: u32 = 0x03;
const VIRTIO_INPUT_CFG_PROP_BITS: u32 = 0x10;
const VIRTIO_INPUT_CFG_EV_BITS: u32 = 0x11;
const VIRTIO_INPUT_CFG_ABS_INFO: u32 = 0x12;

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
struct VirtioInputAbsInfo {
    min: u32,
    max: u32,
    fuzz: u32,
    flat: u32,
    res: u32,
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
struct VirtioInputDevIDs {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct VirtioInputConfig {
    select: u8,
    subsel: u8,
    size: u8,
    reserved: [u8; 5],
    string: [char; 128],
    bitmap: [u8; 128],
    abs: VirtioInputAbsInfo,
    ids: VirtioInputDevIDs,
}

struct VirtioInputEvent {
    event_type: u16,
    code: u16,
    value: u32,
}

struct VhostUserInputThread {
    // input_fd: EventFd,
    vring_worker: Option<Arc<VringWorker>>,
    event_idx: bool,
    kill_evt: EventFd,
}

impl VhostUserInputThread {
    // Create a new virtio input device
    fn new(input_fd: EventFd) -> Result<Self> {
        println!("new VhostUserInputThread");

        Ok(VhostUserInputThread {
            // input_fd,
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
}

struct VhostUserInputBackend {
    thread: Mutex<VhostUserInputThread>,
    config: VirtioInputConfig,
    queues_per_thread: Vec<u64>,
    num_queues: usize,
    queue_size: usize,
}

impl VhostUserInputBackend {
    fn new(input_fd: EventFd, num_queues: usize, queue_size: usize) -> Result<Self> {
        let mut queues_per_thread = Vec::new();

        let thread = Mutex::new(VhostUserInputThread::new(input_fd.try_clone().unwrap())?);

        let config = VirtioInputConfig {
            select: 0,
            subsel: 0,
            size: 0,
            reserved: [0; 5],
            string: ['x'; 128],
            bitmap: [0; 128],
            abs: Default::default(),
            ids: Default::default(),
        };

        Ok(VhostUserInputBackend {
            thread,
            config,
            queues_per_thread,
            num_queues,
            queue_size,
        })
    }
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::std::slice::from_raw_parts((p as *const T) as *const u8, ::std::mem::size_of::<T>())
}

impl VhostUserBackend for VhostUserInputBackend {
    fn num_queues(&self) -> usize {
        println!("num_queues");

        self.num_queues
    }

    fn max_queue_size(&self) -> usize {
        println!("max_queue_size");

        self.queue_size as usize
    }

    fn features(&self) -> u64 {
        println!("features");

        1 << VIRTIO_F_VERSION_1
            | 1 << VIRTIO_INPUT_CFG_ID_NAME
            | 1 << VIRTIO_INPUT_CFG_ID_SERIAL
            | 1 << VIRTIO_INPUT_CFG_ID_DEVIDS
            | 1 << VIRTIO_INPUT_CFG_PROP_BITS
            | 1 << VIRTIO_INPUT_CFG_EV_BITS
            | 1 << VIRTIO_INPUT_CFG_ABS_INFO
            | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        println!("protocol_features");

        VhostUserProtocolFeatures::CONFIG
    }

    fn set_event_idx(&mut self, enabled: bool) {
        println!("set_event_idx");

        self.thread.lock().unwrap().event_idx = enabled;
    }

    fn update_memory(
        &mut self,
        _mem: GuestMemoryAtomic<GuestMemoryMmap>,
    ) -> VhostUserBackendResult<()> {
        println!("update_memory");

        Ok(())
    }

    fn handle_event(
        &self,
        device_event: u16,
        evset: epoll::Events,
        vrings: &[Arc<RwLock<Vring>>],
        thread_id: usize,
    ) -> VhostUserBackendResult<bool> {
        println!("handle event");

        if evset != epoll::Events::EPOLLIN {
            return Err(Error::HandleEventNotEpollIn.into());
        }

        println!("event received: {:#?}", device_event);
        let mut thread = self.thread.lock().unwrap();
        match device_event {
            0 => {
                let mut vring = vrings[0].write().unwrap();
                if thread.event_idx {
                    loop {
                        vring.mut_queue();
                        if !thread.process_queue(&mut vring) {
                            break;
                        }
                    }
                } else {
                    thread.process_queue(&mut vring);
                }

                Ok(false)
            }
            _ => Err(Error::HandleEventUnknownEvent.into()),
        }
    }

    fn get_config(&self, _offset: u32, _size: u32) -> Vec<u8> {
        println!("get config!");

        // unsafe { any_as_u8_slice(self.config.borrow()).to_vec() }
        Vec::new()
    }

    fn set_config(&mut self, _offset: u32, _buf: &[u8]) -> result::Result<(), io::Error> {
        println!("set_config");

        // let mut config_slice = unsafe { any_as_u8_slice(self.config.borrow()) }.to_vec();
        // let data_len = _buf.len() as u32;
        // let config_len = config_slice.len() as u32;
        // if _offset + data_len > config_len {
        //     error!("Failed to write config space");
        //     return Err(io::Error::from_raw_os_error(libc::EINVAL));
        // }

        // let (_, right) = config_slice.split_at_mut(_offset as usize);
        // right.copy_from_slice(&_buf[..]);

        Ok(())
    }

    fn queues_per_thread(&self) -> Vec<u64> {
        println!("queues_per_thread");

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
                .min_values(1),
            // .required(true),
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
    // let socket_path = match cmd_arguments.value_of("socket-path") {
    //     None => {
    //         panic!("no socket-path provided!")
    //     }
    //     Some(path) => path,
    // };

    // Add a new listener on the socket-path to listen for events
    // ** Hard-coded socket for debugging ** 
    let listener = Listener::new("/tmp/vinput.sock", true).unwrap();
    // TODO: Implement logging
    println!("listening on {}", "/tmp/vinput.sock");

    // EventFd for synthetic inputs to the VhostUserInputThread
    let sim_inputs = EventFd::new(EFD_NONBLOCK).unwrap();

    let input_backend = Arc::new(RwLock::new(
        VhostUserInputBackend::new(sim_inputs.try_clone().unwrap(), 1, 1024).unwrap(),
    ));
    println!("VhostUserInputBackend created...");

    let mut daemon =
        VhostUserDaemon::new("vhost-user-input".to_string(), input_backend.clone()).unwrap();
    println!("VhostUserDaemon created...");

    if let Err(e) = daemon.start(listener) {
        error!("Failed to start daemon: {:?}", e);
        process::exit(1);
    }
    println!("VhostUserDaemon started...");

    // // Get vring_workers from the VhostUserInputThread, register listeners on each of them for
    // // synthetic inputs EventFd created earlier
    // let vring_workers = daemon.get_vring_workers();
    // for vring_worker in vring_workers {
    //     // Send dummy data for now
    //     if let Err(e) = vring_worker.register_listener(sim_inputs.as_raw_fd(), epoll::Events::EPOLLIN, 0) {
    //         error!("Failed to register VringWorker: {:?}", e);
    //         process::exit(1)
    //     }
    // }

    if let Err(e) = daemon.wait() {
        error!("Waiting for daemon failed: {:?}", e);
    }
    println!("Waiting complete");

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

    println!("Worked threads closed.");
    process::exit(0);
}
