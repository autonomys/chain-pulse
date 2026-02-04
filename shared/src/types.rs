use sp_runtime::codec::Encode;
use subxt_core::constants::address::Address;

pub struct EventSegmentSize;

impl Address for EventSegmentSize {
    type Target = u32;

    fn pallet_name(&self) -> &str {
        "System"
    }

    fn constant_name(&self) -> &str {
        "EventSegmentSize"
    }
}

pub(crate) fn system_event_segment_key(segment: u32) -> Vec<u8> {
    let a = sp_crypto_hashing::twox_128(b"System");
    let b = sp_crypto_hashing::twox_128(b"EventSegments");
    let c = segment.encode();
    let mut key = vec![];
    key.extend_from_slice(&a);
    key.extend_from_slice(&b);
    key.extend_from_slice(&c);
    key
}
