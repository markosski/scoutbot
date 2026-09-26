use std::io;
use std::thread;
use std::time::Duration;

const SYNC_BYTE: u8 = 0xA5;
const RESPONSE_SYNC_BYTE: u8 = 0x5A;
const CMD_STOP: u8 = 0x25;
const CMD_SCAN: u8 = 0x20;
const SCAN_RESPONSE_TYPE: u8 = 0x81;
const NODE_SIZE: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanPoint {
    pub angle_deg: f32,
    pub distance_mm: f32,
    pub quality: u8,
    pub starts_new_scan: bool,
}

#[derive(Default)]
pub struct ScanDecoder {
    candidate: [u8; NODE_SIZE],
    length: usize,
}

impl ScanDecoder {
    pub fn push(&mut self, byte: u8) -> Option<ScanPoint> {
        self.candidate[self.length] = byte;
        self.length += 1;
        if self.length < NODE_SIZE {
            return None;
        }

        if let Some(point) = decode_node(self.candidate) {
            self.length = 0;
            Some(point)
        } else {
            self.candidate.copy_within(1..NODE_SIZE, 0);
            self.length = NODE_SIZE - 1;
            None
        }
    }
}

pub fn decode_node(node: [u8; NODE_SIZE]) -> Option<ScanPoint> {
    let starts_new_scan = node[0] & 0x01 != 0;
    let inverted_start = node[0] & 0x02 != 0;
    let check_bit = node[1] & 0x01 != 0;
    if starts_new_scan == inverted_start || !check_bit {
        return None;
    }

    let quality = node[0] >> 2;
    let angle_q6 = ((node[1] as u16) >> 1) | ((node[2] as u16) << 7);
    let distance_q2 = u16::from_le_bytes([node[3], node[4]]);
    Some(ScanPoint {
        angle_deg: angle_q6 as f32 / 64.0,
        distance_mm: distance_q2 as f32 / 4.0,
        quality,
        starts_new_scan,
    })
}

pub fn begin_scan(port: &mut dyn serialport::SerialPort) -> io::Result<()> {
    port.write_all(&[SYNC_BYTE, CMD_STOP])?;
    port.flush()?;
    thread::sleep(Duration::from_millis(30));
    port.clear(serialport::ClearBuffer::Input)?;

    port.write_all(&[SYNC_BYTE, CMD_SCAN])?;
    port.flush()?;

    let mut descriptor = [0_u8; 7];
    port.read_exact(&mut descriptor)?;
    validate_scan_descriptor(descriptor)
}

pub fn stop_scan(port: &mut dyn serialport::SerialPort) {
    let _ = port.write_all(&[SYNC_BYTE, CMD_STOP]);
    let _ = port.flush();
}

fn validate_scan_descriptor(descriptor: [u8; 7]) -> io::Result<()> {
    if descriptor[0] != SYNC_BYTE || descriptor[1] != RESPONSE_SYNC_BYTE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected response header {:02X?}", &descriptor[..2]),
        ));
    }
    let packed_size = u32::from_le_bytes(descriptor[2..6].try_into().unwrap());
    let payload_size = packed_size & 0x3fff_ffff;
    let send_mode = packed_size >> 30;
    if payload_size != NODE_SIZE as u32 || send_mode != 1 || descriptor[6] != SCAN_RESPONSE_TYPE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported scan descriptor {descriptor:02X?}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_node(angle_deg: f32, distance_mm: f32, quality: u8, start: bool) -> [u8; 5] {
        let angle_q6 = (angle_deg * 64.0).round() as u16;
        let distance_q2 = (distance_mm * 4.0).round() as u16;
        [
            (quality << 2) | u8::from(start) | (u8::from(!start) << 1),
            ((angle_q6 << 1) as u8) | 1,
            (angle_q6 >> 7) as u8,
            distance_q2 as u8,
            (distance_q2 >> 8) as u8,
        ]
    }

    #[test]
    fn decodes_measurement_node() {
        let point = decode_node(encoded_node(90.0, 1234.0, 42, true)).unwrap();
        assert_eq!(point.angle_deg, 90.0);
        assert_eq!(point.distance_mm, 1234.0);
        assert_eq!(point.quality, 42);
        assert!(point.starts_new_scan);
    }

    #[test]
    fn rejects_bad_sync_and_check_bits() {
        let mut node = encoded_node(10.0, 100.0, 10, false);
        node[0] = (node[0] & !0x03) | 0x03;
        assert!(decode_node(node).is_none());

        let mut node = encoded_node(10.0, 100.0, 10, false);
        node[1] &= !1;
        assert!(decode_node(node).is_none());
    }

    #[test]
    fn decoder_recovers_after_noise() {
        let expected = encoded_node(225.5, 2000.0, 31, false);
        let mut decoder = ScanDecoder::default();
        for byte in [0x00, 0xff, 0x55].into_iter().chain(expected) {
            if let Some(point) = decoder.push(byte) {
                assert_eq!(point.angle_deg, 225.5);
                assert_eq!(point.distance_mm, 2000.0);
                return;
            }
        }
        panic!("decoder did not recover its alignment");
    }

    #[test]
    fn accepts_standard_scan_descriptor() {
        assert!(validate_scan_descriptor([0xa5, 0x5a, 0x05, 0, 0, 0x40, 0x81]).is_ok());
    }
}
