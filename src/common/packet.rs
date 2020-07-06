use bytes::Bytes;

#[derive(Clone, Debug)]
pub struct Packet {
    pub index: usize,
    pub data: Bytes,
}
