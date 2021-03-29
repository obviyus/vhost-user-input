# vhost-user-input
Proof-of-Concept port of vhost-user-input written in Rust

# Current Status
- Device creates a UNIX socket to listen on
- `vhost-user-input` successfully launches a `VhostUserDaemon` and implements `VhostUserInputBackend`
- Can also be identified through QEMU
    - QEMU is able to ping the `features()` and `protocol_features()` methods
    - QEMU is successfully able to invoke `get_config()` and `set_config()` methods on the `vhost-user-input` device
    - Following the above, execution fails due to: `qemu-kvm: Received bad msg size` 
      - most likely due to either invalid `VirtioInputConfig` default values
      - or due to how the structs are converted to a slice through `std::slice::as_raw_parts`

# References:
- https://patchwork.ozlabs.org/project/qemu-devel/cover/20180713130916.4153-1-marcandre.lureau@redhat.com/
- https://www.mail-archive.com/qemu-discuss@nongnu.org/msg04694.html
- https://lists.gnu.org/archive/html/qemu-discuss/2017-02/msg00060.html
- https://patchwork.ozlabs.org/project/qemu-devel/patch/1434372804-19506-1-git-send-email-kraxel@redhat.com/
- https://www.kraxel.org/blog/2015/06/new-member-in-the-virtio-family-input-devices/
- https://docs.huihoo.com/doxygen/linux/kernel/3.7/uapi_2linux_2virtio__config_8h_source.html#l00036
