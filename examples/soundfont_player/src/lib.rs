use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::sync::{Arc, Mutex};

pub struct MyPlugin {
    params: Arc<MyParams>,
    synth: Arc<Mutex<fluidlite::Synth>>,
}

// HACK: fluidlite::Synth is not Sync, thus Arc<fluidlite::Synth> is not Send, which is required for `impl Plugin for MyPlugin`
unsafe impl Send for MyPlugin {}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "gain"]
    gain: FloatParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            synth: Arc::new(Mutex::new(
                fluidlite::Synth::new(fluidlite::Settings::new().unwrap()).unwrap(),
            )),
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(300, 120),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-0.5),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Plugin for MyPlugin {
    const NAME: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const VENDOR: &'static str = "xxx";
    const URL: &'static str = "xxx";
    const EMAIL: &'static str = "xxx";

    // IO ports
    const DEFAULT_INPUT_CHANNELS: u32 = 0;
    const DEFAULT_OUTPUT_CHANNELS: u32 = 2;
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;

    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _bus_config: &BusConfig,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let synth = self.synth.lock().unwrap();
        synth.set_sample_rate(buffer_config.sample_rate);
        // TODO: embed default + choose from dialog UI
        synth
            .sfload("/usr/share/soundfonts/FluidR3_GM.sf2", true)
            .unwrap();
        true
    }

    fn reset(&mut self) {}

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            // TODO: why not `&mut egui_ctx` (e.g. for `egui_ctx.input().consume_key()`) ? https://github.com/BillyDM/egui-baseview/blob/d2512c25bff19c05d73032e5349f3acb03d5da25/src/window.rs#L296
            move |egui_ctx, setter, _state| {
                // TODO: file dialog to choose soundfont
                // TODO: settings for reverb, chorus, bank, patch
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    egui::Grid::new("params")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Gain");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                            ui.end_row();
                        });
                });
            },
        )
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let synth = self.synth.try_lock().unwrap(); // audio thread should not block

        //
        // handle note on/off
        //

        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn {
                    timing: _, // TODO: timing offset
                    voice_id: _,
                    channel,
                    note,
                    velocity,
                } => {
                    synth
                        .note_on(
                            channel as u32,
                            note as u32,
                            denormalize_velocity(velocity) as u32,
                        )
                        .unwrap();
                }
                NoteEvent::NoteOff {
                    timing: _,
                    voice_id: _,
                    channel,
                    note,
                    velocity: _,
                } => {
                    synth.note_off(channel as u32, note as u32).unwrap();
                }
                _ => {
                    nih_dbg!("[WARN] unsupported event: {}", event);
                }
            }
        }

        //
        // synthesize
        //

        assert!(buffer.channels() == 2);
        unsafe {
            synth
                .write_f32(
                    buffer.len(),
                    buffer.as_slice()[0].as_mut_ptr(),
                    0,
                    1,
                    buffer.as_slice()[1].as_mut_ptr(),
                    0,
                    1,
                )
                .unwrap()
        };

        //
        // post process gain
        //

        for samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            for sample in samples {
                *sample *= gain;
            }
        }

        ProcessStatus::Normal
    }
}

fn denormalize_velocity(v: f32) -> f32 {
    (v * 127.0).round().clamp(0.0, 127.0)
}
