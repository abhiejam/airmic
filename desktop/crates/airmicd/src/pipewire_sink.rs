//! PipeWire backend: a virtual source node "airmic" that apps see as a microphone.

use airmic_proto::SAMPLE_RATE;
use anyhow::Context;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS};
use spa::pod::{Object, Pod, Value, serialize::PodSerializer};
use tracing::{info, warn};

use std::sync::Arc;

use crate::sink::{AudioSink, DefaultSource, Level, LevelMeter, SharedBuffer};

pub const NODE_NAME: &str = "airmic";
const DEFAULT_SOURCE_KEY: &str = "default.configured.audio.source";

pub struct PipeWireSink {
    pub set_default_source: bool,
    pub level: Arc<Level>,
}

impl AudioSink for PipeWireSink {
    fn run(self: Box<Self>, buffer: SharedBuffer) -> anyhow::Result<()> {
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None).context("PipeWire main loop")?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None).context("connecting to PipeWire")?;

        let stream = pw::stream::StreamBox::new(
            &core,
            "airmic",
            properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                // Not "Audio/Source/Virtual": WirePlumber 0.4 never configures ports for that class.
                *pw::keys::MEDIA_CLASS => "Audio/Source",
                *pw::keys::NODE_NAME => NODE_NAME,
                *pw::keys::NODE_DESCRIPTION => "AirMic",
                // Ask for 10 ms periods to keep latency low; PipeWire uses the smallest request.
                *pw::keys::NODE_LATENCY => format!("480/{SAMPLE_RATE}"),
            },
        )?;

        // Scratch space allocated once, because `process` runs on the real-time thread.
        let scratch: Vec<i16> = Vec::with_capacity(16 * 1024);
        let _listener = stream
            .add_local_listener_with_user_data((buffer, scratch, self.level, LevelMeter::default()))
            .state_changed(|_, _, old, new| info!("PipeWire stream {old:?} -> {new:?}"))
            .process(|stream, (buffer, scratch, level, meter)| {
                let Some(mut pw_buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let requested = pw_buffer.requested() as usize;
                let data = &mut pw_buffer.datas_mut()[0];
                let Some(bytes) = data.data() else { return };
                let mut n = bytes.len() / 2;
                if requested > 0 {
                    n = n.min(requested);
                }
                scratch.resize(n, 0);
                match buffer.lock() {
                    Ok(mut jb) => jb.read(scratch),
                    Err(_) => scratch.fill(0),
                }
                meter.add(scratch, level);
                for (dst, s) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(scratch.iter()) {
                    *dst = s.to_le_bytes();
                }
                let chunk = data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = 2;
                *chunk.size_mut() = (n * 2) as u32;
            })
            .register()?;

        let mut format = AudioInfoRaw::new();
        format.set_format(AudioFormat::S16LE);
        format.set_rate(SAMPLE_RATE);
        format.set_channels(1);
        let mut position = [0; MAX_CHANNELS];
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
        format.set_position(position);
        let format = PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &Value::Object(Object {
                type_: spa::sys::SPA_TYPE_OBJECT_Format,
                id: spa::sys::SPA_PARAM_EnumFormat,
                properties: format.into(),
            }),
        )
        .context("serializing audio format")?
        .0
        .into_inner();
        let mut params = [Pod::from_bytes(&format).context("audio format pod")?];

        // No AUTOCONNECT: a source waits for apps to link to it.
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )?;
        info!("PipeWire source \"{NODE_NAME}\" created");
        if self.set_default_source {
            match set_default_source() {
                Ok(()) => info!("AirMic set as the default microphone"),
                Err(e) => warn!("could not set the default microphone: {e:#}"),
            }
        }

        mainloop.run();
        anyhow::bail!("PipeWire main loop stopped")
    }
}

/// Reads and sets the default input through `pw-metadata`.
pub struct PipeWireDefault;

impl DefaultSource for PipeWireDefault {
    fn is_default(&self) -> bool {
        let out = std::process::Command::new("pw-metadata")
            .args(["0", DEFAULT_SOURCE_KEY])
            .output();
        out.is_ok_and(|out| {
            configured_source_name(&String::from_utf8_lossy(&out.stdout)).as_deref()
                == Some(NODE_NAME)
        })
    }

    fn make_default(&self) -> anyhow::Result<()> {
        set_default_source()
    }
}

/// Returns the node name in a `pw-metadata` listing line such as
/// `update: id:0 key:'…' value:'{"name":"airmic"}' type:'Spa:String:JSON'`.
fn configured_source_name(listing: &str) -> Option<String> {
    let value = listing.split("value:'").nth(1)?.split("' type:").next()?;
    let value: serde_json::Value = serde_json::from_str(value).ok()?;
    Some(value.get("name")?.as_str()?.to_string())
}

/// Makes AirMic the default input by node name, which survives node id changes across restarts.
fn set_default_source() -> anyhow::Result<()> {
    let value = format!(r#"{{ "name": "{NODE_NAME}" }}"#);
    let out = std::process::Command::new("pw-metadata")
        .args(["0", DEFAULT_SOURCE_KEY, &value, "Spa:String:JSON"])
        .output()
        .context("running pw-metadata")?;
    anyhow::ensure!(
        out.status.success(),
        "pw-metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_configured_source_name() {
        let line = r#"update: id:0 key:'default.configured.audio.source' value:'{ "name": "airmic" }' type:'Spa:String:JSON'"#;
        assert_eq!(configured_source_name(line).as_deref(), Some("airmic"));
        assert_eq!(
            configured_source_name("Found \"default\" metadata 38\n"),
            None
        );
    }
}
