use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::sync::{Arc, Mutex};

pub struct MyPlugin {
    params: Arc<MyParams>,
    synth: Arc<Mutex<oxisynth::Synth>>,
    soundfonts: Arc<Mutex<Vec<oxisynth::SoundFont>>>, // keep independently from `Synth` since it's accessed frequently on gui thread
}

// embed 1KB of simplest soundfont as default
const DEFAULT_SOUNDFONT: &[u8] = include_bytes!("../../../thirdparty/OxiSynth/testdata/sin.sf2");

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "gain"]
    gain: FloatParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        let mut cursor = std::io::Cursor::new(DEFAULT_SOUNDFONT);
        let soundfont = oxisynth::SoundFont::load(&mut cursor).unwrap();
        let mut synth = oxisynth::Synth::default();
        synth.add_font(soundfont.clone(), true);
        Self {
            params: Arc::new(MyParams::default()),
            synth: Arc::new(Mutex::new(synth)),
            soundfonts: Arc::new(Mutex::new(vec![soundfont])),
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

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let soundfonts = self.soundfonts.clone();
        let _synth = self.synth.clone();
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                // TODO: file dialog to choose soundfont (https://github.com/emilk/egui/blob/34f587d1e1cc69146f7a02f20903e4f573030ffd/examples/file_dialog/src/main.rs)
                // TODO: settings for reverb, chorus, bank, patch
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    egui::Grid::new("params")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Gain");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                            ui.end_row();

                            ui.label("Soundfont");
                            if ui.button("Open file…").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_file() {
                                    dbg!(&path);
                                    let mut file = std::fs::File::open(path).unwrap();
                                    let soundfont = oxisynth::SoundFont::load(&mut file).unwrap();
                                    soundfonts.lock().unwrap().push(soundfont);
                                }
                            }
                            ui.end_row();
                        });

                    // TODO: soundfont/bank/preset selector
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
        // TODO: locked on main thread e.g. when loading a new font or changing preset.
        let mut synth = self.synth.try_lock().unwrap();

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
                        .send_event(oxisynth::MidiEvent::NoteOn {
                            channel,
                            key: note,
                            vel: denormalize_velocity(velocity) as u8,
                        })
                        .unwrap();
                }
                NoteEvent::NoteOff {
                    timing: _,
                    voice_id: _,
                    channel,
                    note,
                    velocity: _,
                } => {
                    synth
                        .send_event(oxisynth::MidiEvent::NoteOff { channel, key: note })
                        .unwrap();
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

        for samples in buffer.iter_samples() {
            // params
            let gain = self.params.gain.smoothed.next();

            // write left/right samples
            let mut synth_samples = [0f32; 2];
            synth.write(&mut synth_samples[..]);

            for (synth_sample, sample) in synth_samples.iter().zip(samples) {
                *sample = gain * *synth_sample;
            }
        }

        ProcessStatus::Normal
    }
}

fn denormalize_velocity(v: f32) -> f32 {
    (v * 127.0).round().clamp(0.0, 127.0)
}
