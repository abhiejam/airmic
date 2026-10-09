//! PipeWire backend: a virtual source node "airmic" that apps see as a microphone.

use std::sync::{Arc, OnceLock};

use airmic_proto::SAMPLE_RATE;
use anyhow::Context;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::param::audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS};
use spa::pod::{Object, Pod, Value, serialize::PodSerializer};
use tracing::{info, warn};

use crate::sink::{AudioSink, DefaultSource, SharedBuffer};

pub const NODE_NAME: &str = "airmic";
const DEFAULT_SOURCE_KEY: &str = "default.configured.audio.source";

pub struct PipeWireSink {
    /// Set to make AirMic the default input once the node exists.
    pub default_source: Option<Arc<PipeWireDefault>>,
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
            .add_local_listener_with_user_data((buffer, scratch))
            .state_changed(|_, _, old, new| info!("PipeWire stream {old:?} -> {new:?}"))
            .process(|stream, (buffer, scratch)| {
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
        if let Some(default_source) = &self.default_source {
            match default_source.make_default() {
                Ok(()) => info!("AirMic set as the default microphone"),
                Err(e) => warn!("could not set the default microphone: {e:#}"),
            }
        }

        mainloop.run();
        anyhow::bail!("PipeWire main loop stopped")
    }
}

/// Reads and sets the default input through `pw-metadata`. Remembers the default it replaced,
/// so `restore_previous` can put it back on exit.
#[derive(Default)]
pub struct PipeWireDefault {
    /// The configured default before AirMic first took over, `None` inside when none was set.
    previous: OnceLock<Option<String>>,
}

impl DefaultSource for PipeWireDefault {
    fn is_default(&self) -> bool {
        read_configured_source().is_ok_and(|value| value.as_deref().is_some_and(names_airmic))
    }

    fn make_default(&self) -> anyhow::Result<()> {
        if self.previous.get().is_none() {
            let _ = self.previous.set(read_configured_source()?);
        }
        let value = format!(r#"{{ "name": "{NODE_NAME}" }}"#);
        run_pw_metadata(&["0", DEFAULT_SOURCE_KEY, &value, "Spa:String:JSON"]).map(drop)
    }
}

impl PipeWireDefault {
    /// Puts back the default input AirMic replaced, unless the user has picked another since.
    pub fn restore_previous(&self) -> anyhow::Result<()> {
        let Some(previous) = self.previous.get() else {
            return Ok(());
        };
        let current = read_configured_source()?;
        match plan_default_restore(previous.as_deref(), current.as_deref()) {
            Restore::Keep => Ok(()),
            Restore::Clear => run_pw_metadata(&["-d", "0", DEFAULT_SOURCE_KEY]).map(drop),
            Restore::Set(value) => {
                run_pw_metadata(&["0", DEFAULT_SOURCE_KEY, value, "Spa:String:JSON"]).map(drop)
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum Restore<'a> {
    Keep,
    Clear,
    Set(&'a str),
}

/// Decides what to write back on exit from the configured default before start and now.
fn plan_default_restore<'a>(previous: Option<&'a str>, current: Option<&str>) -> Restore<'a> {
    if !current.is_some_and(names_airmic) {
        return Restore::Keep;
    }
    match previous {
        Some(value) if !names_airmic(value) => Restore::Set(value),
        // Nothing was set, or AirMic was left over from a crash: let WirePlumber choose.
        _ => Restore::Clear,
    }
}

/// Returns the raw JSON value of the configured default input, or `None` when none is set.
fn read_configured_source() -> anyhow::Result<Option<String>> {
    let listing = run_pw_metadata(&["0", DEFAULT_SOURCE_KEY])?;
    Ok(configured_source_value(&listing).map(str::to_string))
}

/// Returns the value in a `pw-metadata` listing line such as
/// `update: id:0 key:'…' value:'{"name":"airmic"}' type:'Spa:String:JSON'`.
fn configured_source_value(listing: &str) -> Option<&str> {
    listing.split("value:'").nth(1)?.split("' type:").next()
}

fn names_airmic(value: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(value)
        .is_ok_and(|v| v.get("name").and_then(|n| n.as_str()) == Some(NODE_NAME))
}

/// Runs `pw-metadata` with `args` and returns its stdout.
fn run_pw_metadata(args: &[&str]) -> anyhow::Result<String> {
    let out = std::process::Command::new("pw-metadata")
        .args(args)
        .output()
        .context("running pw-metadata")?;
    anyhow::ensure!(
        out.status.success(),
        "pw-metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_configured_source_value() {
        let line = r#"update: id:0 key:'default.configured.audio.source' value:'{ "name": "airmic" }' type:'Spa:String:JSON'"#;
        let value = configured_source_value(line).unwrap();
        assert_eq!(value, r#"{ "name": "airmic" }"#);
        assert!(names_airmic(value));
        assert!(!names_airmic(r#"{"name":"alsa_input.usb-mic"}"#));
        assert_eq!(
            configured_source_value("Found \"default\" metadata 38\n"),
            None
        );
    }

    const AIRMIC: &str = r#"{"name":"airmic"}"#;
    const USB_MIC: &str = r#"{"name":"alsa_input.usb-mic"}"#;

    #[test]
    fn restores_the_previous_default_while_airmic_is_still_default() {
        assert_eq!(
            plan_default_restore(Some(USB_MIC), Some(AIRMIC)),
            Restore::Set(USB_MIC)
        );
    }

    #[test]
    fn keeps_a_default_the_user_picked_while_the_daemon_ran() {
        let other = r#"{"name":"bluez_input.headset"}"#;
        assert_eq!(
            plan_default_restore(Some(USB_MIC), Some(other)),
            Restore::Keep
        );
        assert_eq!(plan_default_restore(None, None), Restore::Keep);
    }

    #[test]
    fn clears_the_default_when_none_was_set_or_airmic_was_left_from_a_crash() {
        assert_eq!(plan_default_restore(None, Some(AIRMIC)), Restore::Clear);
        assert_eq!(
            plan_default_restore(Some(AIRMIC), Some(AIRMIC)),
            Restore::Clear
        );
    }
}
