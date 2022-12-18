use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::{f32::consts::TAU, sync::Arc};

pub struct MyPlugin {
    params: Arc<MyParams>,
    sample_phase: f32,
    envelope_phase: f32,
}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "bpm"]
    bpm: IntParam,

    #[id = "gain"]
    gain: FloatParam,

    #[id = "note"]
    note: IntParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            sample_phase: 0.0,
            envelope_phase: 0.0,
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(300, 100),
            bpm: IntParam::new("BPM", 150, IntRange::Linear { min: 1, max: 300 }),
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
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
            note: IntParam::new(
                "Note",
                // A5
                69 + 12,
                // C2..C6
                IntRange::Linear {
                    min: 60 - 12 * 2,
                    max: 60 + 12 * 2,
                },
            )
            .with_value_to_string(formatters::v2s_i32_note_formatter())
            .with_string_to_value(formatters::s2v_i32_note_formatter()),
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
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;

    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    egui::Grid::new("params")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("BPM");
                            ui.add(widgets::ParamSlider::for_param(&params.bpm, setter));
                            ui.end_row();

                            ui.label("Gain");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                            ui.end_row();

                            ui.label("Note");
                            ui.add(widgets::ParamSlider::for_param(&params.note, setter));
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
        let sample_rate = context.transport().sample_rate;

        // params
        let bpm = self.params.bpm.value() as f32;
        let note = self.params.note.value() as u8;
        let note_freq = nih_plug::util::midi_note_to_freq(note);

        for samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            let sine = next_sine(&mut self.sample_phase, note_freq / sample_rate);
            let envelope = next_envelope(
                &mut self.envelope_phase,
                0.005,
                0.05,
                60.0 / bpm,
                1.0 / sample_rate,
            );
            let value = gain * envelope * sine;
            for sample in samples {
                *sample = value;
            }
        }

        ProcessStatus::Normal
    }
}

fn next_sine(phase: &mut f32, delta: f32) -> f32 {
    let value = (TAU * *phase).sin();
    *phase += delta;
    while *phase >= 1.0 {
        *phase -= 1.0;
    }
    value
}

fn next_envelope(phase: &mut f32, attack: f32, release: f32, interval: f32, delta: f32) -> f32 {
    let value = if *phase < attack {
        *phase / attack
    } else if *phase < attack + release {
        1.0 - (*phase - attack) / release
    } else {
        0.0
    };
    *phase += delta;
    while *phase >= interval {
        *phase -= interval;
    }
    value
}
