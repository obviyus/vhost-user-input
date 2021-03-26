use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde::{Deserialize, Deserializer};
use vm_memory::ByteValued;

#[derive(Copy, Clone, Debug, Default, Deserialize)]
#[repr(C, packed)]
pub struct VirtioInputAbsInfo {
    min: u32,
    max: u32,
    fuzz: u32,
    flat: u32,
    res: u32,
}

impl Serialize for VirtioInputAbsInfo {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let min = self.min;
        let max = self.max;
        let fuzz = self.fuzz;
        let flat = self.flat;
        let res = self.res;

        let mut virtio_input_devids = serializer.serialize_struct("VirtioInputDevIDs", 4)?;
        virtio_input_devids.serialize_field("min", &min);
        virtio_input_devids.serialize_field("max", &max);
        virtio_input_devids.serialize_field("fuzz", &fuzz);
        virtio_input_devids.serialize_field("flat", &flat);
        virtio_input_devids.serialize_field("res", &res);

        virtio_input_devids.end()
    }
}

unsafe impl ByteValued for VirtioInputAbsInfo {}

#[derive(Copy, Clone, Debug, Default, Deserialize)]
#[repr(C, packed)]
pub struct VirtioInputDevIDs {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

impl Serialize for VirtioInputDevIDs {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let bustype = self.bustype;
        let vendor = self.vendor;
        let product = self.product;
        let version = self.version;

        let mut virtio_input_devids = serializer.serialize_struct("VirtioInputDevIDs", 4)?;
        virtio_input_devids.serialize_field("bustype", &bustype);
        virtio_input_devids.serialize_field("vendor", &vendor);
        virtio_input_devids.serialize_field("product", &product);
        virtio_input_devids.serialize_field("version", &version);

        virtio_input_devids.end()
    }
}

unsafe impl ByteValued for VirtioInputDevIDs {}

#[derive(Copy, Clone)]
union U {
    string: [char; 128],
    bitmap: [u8; 128],
    abs: VirtioInputAbsInfo,
    ids: VirtioInputDevIDs,
}

#[derive(Copy, Clone, Deserialize)]
#[repr(C, packed)]
pub struct VirtioInputConfig {
    pub select: u8,
    pub subsel: u8,
    pub size: u8,
    pub reserved: [u8; 5],
    // pub u: U,
}

impl Serialize for VirtioInputConfig {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let select = self.select;
        let subsel = self.subsel;
        let size = self.size;
        let reserved = self.reserved;

        let mut virtio_input_config = serializer.serialize_struct("VirtioInputConfig", 60)?;
        virtio_input_config.serialize_field("select", &select);
        virtio_input_config.serialize_field("subsel", &subsel);
        virtio_input_config.serialize_field("size", &size);
        virtio_input_config.serialize_field("reserved", &reserved);

        virtio_input_config.end()
    }
}

unsafe impl ByteValued for VirtioInputConfig {}

pub struct VirtioInputEvent {
    event_type: u16,
    code: u16,
    value: u32,
}

impl Default for VirtioInputConfig {
    fn default() -> Self {
        VirtioInputConfig {
            select: 0,
            subsel: 0,
            size: 0,
            reserved: [0; 5],
            // u: U {
            //
            // }
        }
    }
}
