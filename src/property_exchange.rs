//! Minimal MIDI-CI Property Exchange (PE) framing.
//!
//! Handles Set Property Inquiry (sub-ID2 = 0x36) and replies with ACK (0x37).
//! Spec reference: MIDI-CI 1.2, §7 Property Exchange.
//!
//! SysEx7 message layout:
//!   F0 7E <device_id> 0D <sub_id2> <ci_version>
//!   <source_muid: 4 bytes> <dest_muid: 4 bytes>
//!   <request_id>
//!   <header_len_lo> <header_len_hi>
//!   <header_data...>
//!   <num_chunks_lo> <num_chunks_hi>
//!   <chunk_num_lo> <chunk_num_hi>
//!   <body_len_lo> <body_len_hi>
//!   <body_data...>
//!   F7

use heapless::Vec;

const UNIVERSAL_SYSEX: u8 = 0x7E;
const SUB_ID1_MIDI_CI: u8 = 0x0D;

/// Sub-ID2: Inquiry Get Property Data (MIDI-CI 1.2)
pub const PE_GET_INQUIRY: u8 = 0x34;
/// Sub-ID2: Reply to Get Property Data
pub const PE_GET_REPLY: u8 = 0x35;
/// Sub-ID2: Inquiry Set Property Data (MIDI-CI 1.2)
pub const PE_SET_INQUIRY: u8 = 0x36;
/// Sub-ID2: Reply to Set Property Data
pub const PE_SET_REPLY: u8 = 0x37;

const CI_VERSION: u8 = 0x02;

/// Check if a SysEx buffer is a MIDI-CI message.
pub fn is_ci_message(buf: &[u8]) -> bool {
    buf.len() >= 15 && buf[0] == 0xF0 && buf[1] == UNIVERSAL_SYSEX && buf[3] == SUB_ID1_MIDI_CI
}

/// Check if the message is a Set Property Inquiry.
pub fn is_set_property(buf: &[u8]) -> bool {
    is_ci_message(buf) && buf[4] == PE_SET_INQUIRY
}

/// Extract the request_id field.
pub fn request_id(buf: &[u8]) -> u8 {
    buf[14]
}

/// Extract the source MUID (4 bytes).
pub fn source_muid(buf: &[u8]) -> [u8; 4] {
    [buf[6], buf[7], buf[8], buf[9]]
}

/// Parsed Set Property Inquiry with resource identifier and body.
pub struct SetPropertyData<'a> {
    /// Resource identifier (preset index) from header.
    pub resource: u8,
    /// Body payload.
    pub body: &'a [u8],
}

/// Extract resource identifier and body from a Set Property Inquiry.
pub fn extract_set_property(buf: &[u8]) -> Option<SetPropertyData<'_>> {
    if !is_set_property(buf) {
        return None;
    }
    if buf.len() < 16 {
        return None;
    }

    let mut pos = 15;

    // header_len (2 bytes, 7-bit LSB encoding)
    if pos + 2 > buf.len() {
        return None;
    }
    let header_len = (buf[pos] as usize) | ((buf[pos + 1] as usize) << 7);
    pos += 2;

    // resource = first header byte (preset index), or 0 if no header
    let resource = if header_len > 0 && pos < buf.len() {
        buf[pos]
    } else {
        0
    };
    pos += header_len;

    // num_chunks + chunk_num (4 bytes)
    if pos + 4 > buf.len() {
        return None;
    }
    pos += 4;

    // body_len (2 bytes, 7-bit LSB encoding)
    if pos + 2 > buf.len() {
        return None;
    }
    let body_len = (buf[pos] as usize) | ((buf[pos + 1] as usize) << 7);
    pos += 2;

    if pos + body_len > buf.len() {
        return None;
    }
    Some(SetPropertyData {
        resource,
        body: &buf[pos..pos + body_len],
    })
}

/// Legacy helper: extract just the body (ignores resource).
pub fn extract_body(buf: &[u8]) -> Option<&[u8]> {
    extract_set_property(buf).map(|d| d.body)
}

/// Build a Set Property Reply (ACK).
pub fn build_set_reply(device_muid: [u8; 4], dest_muid: [u8; 4], req_id: u8) -> Vec<u8, 32> {
    let mut msg: Vec<u8, 32> = Vec::new();
    let _ = msg.push(0xF0);
    let _ = msg.push(UNIVERSAL_SYSEX);
    let _ = msg.push(0x7F); // device_id: function block
    let _ = msg.push(SUB_ID1_MIDI_CI);
    let _ = msg.push(PE_SET_REPLY);
    let _ = msg.push(CI_VERSION);
    for &b in &device_muid {
        let _ = msg.push(b);
    }
    for &b in &dest_muid {
        let _ = msg.push(b);
    }
    let _ = msg.push(req_id);
    // header_len = 0
    let _ = msg.push(0x00);
    let _ = msg.push(0x00);
    // num_chunks = 1, chunk_num = 1
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    // body_len = 0
    let _ = msg.push(0x00);
    let _ = msg.push(0x00);
    let _ = msg.push(0xF7);
    msg
}

/// Build a Set Property Inquiry message (CLI → device).
/// `resource` is the preset index (carried in header).
/// `body` must contain only 7-bit safe bytes.
pub fn build_set_inquiry(
    source_muid: [u8; 4],
    dest_muid: [u8; 4],
    req_id: u8,
    resource: u8,
    body: &[u8],
) -> Vec<u8, 350> {
    let mut msg: Vec<u8, 350> = Vec::new();
    let _ = msg.push(0xF0);
    let _ = msg.push(UNIVERSAL_SYSEX);
    let _ = msg.push(0x7F);
    let _ = msg.push(SUB_ID1_MIDI_CI);
    let _ = msg.push(PE_SET_INQUIRY);
    let _ = msg.push(CI_VERSION);
    for &b in &source_muid {
        let _ = msg.push(b);
    }
    for &b in &dest_muid {
        let _ = msg.push(b);
    }
    let _ = msg.push(req_id);
    // header_len = 1 (resource byte)
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(resource);
    // num_chunks = 1, chunk_num = 1
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    // body (mcoded7 encoded)
    let mut encoded_body = [0u8; 300];
    let enc_len = encode_mcoded7(body, &mut encoded_body);
    let _ = msg.push((enc_len & 0x7F) as u8);
    let _ = msg.push(((enc_len >> 7) & 0x7F) as u8);
    for &b in &encoded_body[..enc_len] {
        let _ = msg.push(b);
    }
    let _ = msg.push(0xF7);
    msg
}

/// Check if the message is a Get Property Inquiry.
pub fn is_get_property(buf: &[u8]) -> bool {
    is_ci_message(buf) && buf[4] == PE_GET_INQUIRY
}

/// Check if the message is a Get Property Reply.
pub fn is_get_reply(buf: &[u8]) -> bool {
    is_ci_message(buf) && buf[4] == PE_GET_REPLY
}

/// Extract body from a Get Property Reply.
pub fn extract_get_body(buf: &[u8]) -> Option<&[u8]> {
    if !is_get_reply(buf) || buf.len() < 16 {
        return None;
    }
    let mut pos = 15;
    if pos + 2 > buf.len() {
        return None;
    }
    let header_len = (buf[pos] as usize) | ((buf[pos + 1] as usize) << 7);
    pos += 2 + header_len;
    // num_chunks + chunk_num
    if pos + 4 > buf.len() {
        return None;
    }
    pos += 4;
    // body_len
    if pos + 2 > buf.len() {
        return None;
    }
    let body_len = (buf[pos] as usize) | ((buf[pos + 1] as usize) << 7);
    pos += 2;
    if pos + body_len > buf.len() {
        return None;
    }
    Some(&buf[pos..pos + body_len])
}

/// Extract the resource identifier from a Get Property Inquiry.
pub fn extract_get_resource(buf: &[u8]) -> Option<u8> {
    if !is_get_property(buf) || buf.len() < 16 {
        return None;
    }
    let pos = 15;
    if pos + 2 > buf.len() {
        return None;
    }
    let header_len = (buf[pos] as usize) | ((buf[pos + 1] as usize) << 7);
    if header_len > 0 && pos + 2 < buf.len() {
        Some(buf[pos + 2])
    } else {
        Some(0)
    }
}

/// Build a Get Property Inquiry message (CLI → device).
pub fn build_get_inquiry(
    source_muid: [u8; 4],
    dest_muid: [u8; 4],
    req_id: u8,
    resource: u8,
) -> Vec<u8, 32> {
    let mut msg: Vec<u8, 32> = Vec::new();
    let _ = msg.push(0xF0);
    let _ = msg.push(UNIVERSAL_SYSEX);
    let _ = msg.push(0x7F);
    let _ = msg.push(SUB_ID1_MIDI_CI);
    let _ = msg.push(PE_GET_INQUIRY);
    let _ = msg.push(CI_VERSION);
    for &b in &source_muid {
        let _ = msg.push(b);
    }
    for &b in &dest_muid {
        let _ = msg.push(b);
    }
    let _ = msg.push(req_id);
    // header_len = 1 (resource byte)
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(resource);
    // num_chunks = 1, chunk_num = 1
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    // body_len = 0
    let _ = msg.push(0x00);
    let _ = msg.push(0x00);
    let _ = msg.push(0xF7);
    msg
}

/// Build a Get Property Reply with body data.
pub fn build_get_reply(
    device_muid: [u8; 4],
    dest_muid: [u8; 4],
    req_id: u8,
    resource: u8,
    body: &[u8],
) -> Vec<u8, 350> {
    let mut msg: Vec<u8, 350> = Vec::new();
    let _ = msg.push(0xF0);
    let _ = msg.push(UNIVERSAL_SYSEX);
    let _ = msg.push(0x7F);
    let _ = msg.push(SUB_ID1_MIDI_CI);
    let _ = msg.push(PE_GET_REPLY);
    let _ = msg.push(CI_VERSION);
    for &b in &device_muid {
        let _ = msg.push(b);
    }
    for &b in &dest_muid {
        let _ = msg.push(b);
    }
    let _ = msg.push(req_id);
    // header_len = 1 (resource byte)
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(resource);
    // num_chunks = 1, chunk_num = 1
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    let _ = msg.push(0x01);
    let _ = msg.push(0x00);
    // body (mcoded7 encoded)
    let mut encoded_body = [0u8; 300];
    let enc_len = encode_mcoded7(body, &mut encoded_body);
    let _ = msg.push((enc_len & 0x7F) as u8);
    let _ = msg.push(((enc_len >> 7) & 0x7F) as u8);
    for &b in &encoded_body[..enc_len] {
        let _ = msg.push(b);
    }
    let _ = msg.push(0xF7);
    msg
}

/// Encode data using mcoded7 (MIDI-CI spec): every 7 input bytes → 8 output bytes.
/// The first byte of each group carries the high bits (bit 7) of the following 7 bytes.
/// Returns the number of bytes written to `out`.
pub fn encode_mcoded7(data: &[u8], out: &mut [u8]) -> usize {
    let mut pos = 0;
    for chunk in data.chunks(7) {
        let mut high_bits: u8 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            if b & 0x80 != 0 {
                high_bits |= 1 << i;
            }
        }
        out[pos] = high_bits;
        pos += 1;
        for &b in chunk {
            out[pos] = b & 0x7F;
            pos += 1;
        }
    }
    pos
}

/// Decode mcoded7 data back to original bytes.
/// Returns the number of bytes written to `out`.
pub fn decode_mcoded7(data: &[u8], out: &mut [u8]) -> usize {
    let mut pos = 0;
    let mut i = 0;
    while i < data.len() {
        let high_bits = data[i];
        i += 1;
        for bit in 0..7 {
            if i >= data.len() {
                break;
            }
            out[pos] = data[i] | (if high_bits & (1 << bit) != 0 { 0x80 } else { 0 });
            pos += 1;
            i += 1;
        }
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_set_property() {
        let payload = b"hello";
        let msg = build_set_inquiry(
            [0x10, 0x20, 0x30, 0x40],
            [0x01, 0x02, 0x03, 0x04],
            0x07,
            3,
            payload,
        );

        assert!(is_ci_message(&msg));
        assert!(is_set_property(&msg));
        assert_eq!(request_id(&msg), 0x07);
        assert_eq!(source_muid(&msg), [0x10, 0x20, 0x30, 0x40]);

        let data = extract_set_property(&msg).unwrap();
        assert_eq!(data.resource, 3);
        let mut decoded = [0u8; 32];
        let dec_len = decode_mcoded7(data.body, &mut decoded);
        assert_eq!(&decoded[..dec_len], b"hello");
    }

    #[test]
    fn not_ci_for_opendeck() {
        let buf = [0xF0, 0x00, 0x53, 0x43, 0x00, 0x00, 0x01, 0xF7];
        assert!(!is_ci_message(&buf));
    }

    #[test]
    fn reply_structure() {
        let reply = build_set_reply([0x01, 0x02, 0x03, 0x04], [0x10, 0x20, 0x30, 0x40], 0x07);
        assert_eq!(reply[0], 0xF0);
        assert_eq!(reply[4], PE_SET_REPLY);
        assert_eq!(reply[14], 0x07);
        assert_eq!(*reply.last().unwrap(), 0xF7);
    }

    #[test]
    fn extract_body_with_header() {
        let mut msg: Vec<u8, 128> = Vec::new();
        let _ = msg.push(0xF0);
        let _ = msg.push(UNIVERSAL_SYSEX);
        let _ = msg.push(0x7F);
        let _ = msg.push(SUB_ID1_MIDI_CI);
        let _ = msg.push(PE_SET_INQUIRY);
        let _ = msg.push(CI_VERSION);
        for &b in &[0x10, 0x20, 0x30, 0x40, 0x01, 0x02, 0x03, 0x04] {
            let _ = msg.push(b);
        }
        let _ = msg.push(0x01); // request_id
                                // header_len = 3
        let _ = msg.push(0x03);
        let _ = msg.push(0x00);
        for &b in b"abc" {
            let _ = msg.push(b);
        }
        // chunks
        let _ = msg.push(0x01);
        let _ = msg.push(0x00);
        let _ = msg.push(0x01);
        let _ = msg.push(0x00);
        // body_len = 4
        let _ = msg.push(0x04);
        let _ = msg.push(0x00);
        for &b in b"data" {
            let _ = msg.push(b);
        }
        let _ = msg.push(0xF7);

        assert_eq!(extract_body(&msg).unwrap(), b"data");
    }

    #[test]
    fn get_property_roundtrip() {
        let inquiry =
            build_get_inquiry([0x10, 0x20, 0x30, 0x40], [0x01, 0x02, 0x03, 0x04], 0x05, 7);
        assert!(is_get_property(&inquiry));
        assert!(!is_set_property(&inquiry));
        assert_eq!(extract_get_resource(&inquiry), Some(7));
        assert_eq!(request_id(&inquiry), 0x05);
    }

    #[test]
    fn get_reply_body_extraction() {
        let body = b"preset data here";
        let reply = build_get_reply(
            [0x01, 0x02, 0x03, 0x04],
            [0x10, 0x20, 0x30, 0x40],
            0x03,
            2,
            body,
        );
        assert!(is_get_reply(&reply));
        let raw = extract_get_body(&reply).unwrap();
        let mut decoded = [0u8; 32];
        let dec_len = decode_mcoded7(raw, &mut decoded);
        assert_eq!(&decoded[..dec_len], body);
    }

    #[test]
    fn mcoded7_roundtrip_short() {
        let data = [0x80, 0xFF, 0x00, 0x7F];
        let mut encoded = [0u8; 256];
        let enc_len = encode_mcoded7(&data, &mut encoded);
        let mut decoded = [0u8; 256];
        let dec_len = decode_mcoded7(&encoded[..enc_len], &mut decoded);
        assert_eq!(&decoded[..dec_len], &data);
    }

    #[test]
    fn mcoded7_roundtrip_exact_7() {
        let data = [0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86];
        let mut encoded = [0u8; 256];
        let enc_len = encode_mcoded7(&data, &mut encoded);
        assert_eq!(enc_len, 8); // 7 input → 8 output
        let mut decoded = [0u8; 256];
        let dec_len = decode_mcoded7(&encoded[..enc_len], &mut decoded);
        assert_eq!(&decoded[..dec_len], &data);
    }

    #[test]
    fn mcoded7_roundtrip_14_bytes() {
        let data = [0xFF; 14];
        let mut encoded = [0u8; 256];
        let enc_len = encode_mcoded7(&data, &mut encoded);
        assert_eq!(enc_len, 16); // 14 → 2 groups of 8
        let mut decoded = [0u8; 256];
        let dec_len = decode_mcoded7(&encoded[..enc_len], &mut decoded);
        assert_eq!(&decoded[..dec_len], &data);
    }

    #[test]
    fn mcoded7_all_7bit_safe_is_passthrough_with_prefix() {
        let data = [0x01, 0x7F, 0x00, 0x55];
        let mut encoded = [0u8; 256];
        let enc_len = encode_mcoded7(&data, &mut encoded);
        // High bits byte should be 0 (no high bits set)
        assert_eq!(encoded[0], 0x00);
        assert_eq!(&encoded[1..5], &data);
        assert_eq!(enc_len, 5);
    }

    #[test]
    fn mcoded7_custom_rgb_color() {
        // Simulate a postcard-serialized Custom(255, 128, 0)
        let data = [0xFF, 0x80, 0x00];
        let mut encoded = [0u8; 256];
        let enc_len = encode_mcoded7(&data, &mut encoded);
        let mut decoded = [0u8; 256];
        let dec_len = decode_mcoded7(&encoded[..enc_len], &mut decoded);
        assert_eq!(&decoded[..dec_len], &data);
    }
}
