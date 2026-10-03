//! AirMic wire protocol v1: UDP audio header and TCP control messages (docs/protocol.md).

use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;
pub const MAGIC: [u8; 2] = *b"AM";
pub const HEADER_LEN: usize = 16;
pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 480;
pub const FRAME_BYTES: usize = FRAME_SAMPLES * 2;
pub const CONTROL_PORT: u16 = 47_800;
pub const AUDIO_PORT: u16 = 47_801;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub muted: bool,
    pub codec: u8,
    pub session_id: u32,
    pub sequence: u32,
    pub timestamp: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    TooShort,
    BadMagic,
    BadVersion,
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0; HEADER_LEN];
        b[..2].copy_from_slice(&MAGIC);
        b[2] = VERSION;
        b[3] = u8::from(self.muted) | (self.codec & 0b111) << 1;
        b[4..8].copy_from_slice(&self.session_id.to_be_bytes());
        b[8..12].copy_from_slice(&self.sequence.to_be_bytes());
        b[12..16].copy_from_slice(&self.timestamp.to_be_bytes());
        b
    }

    /// Decodes the first 16 bytes of `packet`. Reserved flag bits are ignored.
    pub fn decode(packet: &[u8]) -> Result<Header, HeaderError> {
        let b: &[u8; HEADER_LEN] = packet
            .get(..HEADER_LEN)
            .and_then(|s| s.try_into().ok())
            .ok_or(HeaderError::TooShort)?;
        if b[..2] != MAGIC {
            return Err(HeaderError::BadMagic);
        }
        if b[2] != VERSION {
            return Err(HeaderError::BadVersion);
        }
        let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Header {
            muted: b[3] & 1 == 1,
            codec: (b[3] >> 1) & 0b111,
            session_id: be(4),
            sequence: be(8),
            timestamp: be(12),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnsupportedVersion,
    Busy,
    BadCode,
    PairLocked,
    BadMessage,
}

/// One control channel message: a JSON object per line, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello {
        v: u32,
        phone_id: String,
        phone_name: String,
    },
    PairRequired,
    Pair {
        code: String,
    },
    Paired {
        token: String,
    },
    Auth {
        token: String,
    },
    Ready {
        session_id: u32,
        udp_port: u16,
        sample_rate: u32,
    },
    Mute {
        on: bool,
    },
    Stats {
        loss_pct: f64,
        jitter_ms: f64,
        latency_ms: f64,
    },
    Transcript {
        text: String,
        #[serde(rename = "final")]
        is_final: bool,
    },
    Ping,
    Pong,
    Bye,
    Error {
        code: ErrorCode,
        message: String,
    },
    /// Any `type` this version does not know. Receivers ignore it.
    #[serde(other, skip_serializing)]
    Unknown,
}

impl Message {
    /// Returns the message as one JSON line, `\n` included.
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("message serializes");
        line.push('\n');
        line
    }

    pub fn from_line(line: &str) -> serde_json::Result<Message> {
        serde_json::from_str(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vectors() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../docs/protocol/vectors.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn headers_match_vectors() {
        for v in vectors()["headers"].as_array().unwrap() {
            let f = &v["fields"];
            let expected = Header {
                muted: f["muted"].as_bool().unwrap(),
                codec: f["codec"].as_u64().unwrap() as u8,
                session_id: f["session_id"].as_u64().unwrap() as u32,
                sequence: f["sequence"].as_u64().unwrap() as u32,
                timestamp: f["timestamp"].as_u64().unwrap() as u32,
            };
            let bytes = hex(v["hex"].as_str().unwrap());
            assert_eq!(Header::decode(&bytes), Ok(expected), "{}", v["name"]);
            if v["decode_only"].as_bool() != Some(true) {
                assert_eq!(expected.encode().to_vec(), bytes, "{}", v["name"]);
            }
        }
    }

    #[test]
    fn invalid_headers_are_rejected() {
        for v in vectors()["invalid_headers"].as_array().unwrap() {
            let err = match v["error"].as_str().unwrap() {
                "bad_magic" => HeaderError::BadMagic,
                "bad_version" => HeaderError::BadVersion,
                "too_short" => HeaderError::TooShort,
                other => panic!("unknown error {other}"),
            };
            assert_eq!(Header::decode(&hex(v["hex"].as_str().unwrap())), Err(err));
        }
    }

    #[test]
    fn unsupported_codec_is_decoded() {
        for v in vectors()["unsupported_codec"].as_array().unwrap() {
            let h = Header::decode(&hex(v["hex"].as_str().unwrap())).unwrap();
            assert_eq!(u64::from(h.codec), v["codec"].as_u64().unwrap());
        }
    }

    #[test]
    fn pcm_payload_is_little_endian() {
        let p = &vectors()["pcm_payload"];
        let bytes: Vec<u8> = p["samples"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| (s.as_i64().unwrap() as i16).to_le_bytes())
            .collect();
        assert_eq!(bytes, hex(p["hex"].as_str().unwrap()));
    }

    #[test]
    fn messages_round_trip() {
        for v in vectors()["messages"].as_array().unwrap() {
            let json = v["json"].as_str().unwrap();
            let msg = Message::from_line(json).unwrap();
            let line = msg.to_line();
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            let ours: Value = serde_json::from_str(&line).unwrap();
            let theirs: Value = serde_json::from_str(json).unwrap();
            assert_eq!(ours, theirs, "{}", v["type"]);
        }
    }

    #[test]
    fn unknown_fields_and_types_are_tolerated() {
        for v in vectors()["messages_tolerated"].as_array().unwrap() {
            let msg = Message::from_line(v["json"].as_str().unwrap()).unwrap();
            match v["parses_as"].as_str() {
                Some(expected) => assert_eq!(msg, Message::from_line(expected).unwrap()),
                None => assert_eq!(msg, Message::Unknown),
            }
        }
    }
}
