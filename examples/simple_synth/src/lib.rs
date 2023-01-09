use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::{
    f32::consts::TAU,
    sync::{
        atomic::{AtomicIsize, Ordering},
        Arc,
    },
};

pub struct MyPlugin {
    params: Arc<MyParams>,
    oscillator: Oscillator,
    envelop: Envelope,
}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "gain"]
    gain: FloatParam,

    #[id = "attack"]
    attack: FloatParam,

    #[id = "release"]
    release: FloatParam,

    #[id = "note"]
    note: IntParam,

    note_state: Arc<NoteState>,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            oscillator: Oscillator::new(),
            envelop: Envelope::new(),
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(300, 150),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-5.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-20.0),
                    max: util::db_to_gain(10.0),
                    factor: FloatRange::gain_skew_factor(-20.0, 10.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            attack: FloatParam::new(
                "Attack",
                0.001,
                // TODO: skew?
                FloatRange::Linear {
                    min: 0.001,
                    max: 2.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(v2s_f32_scale(1000.0, 0))
            .with_string_to_value(s2v_f32_scale(1000.0, " ms".to_owned())),

            release: FloatParam::new(
                "Release",
                0.1,
                FloatRange::Linear {
                    min: 0.001,
                    max: 2.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(v2s_f32_scale(1000.0, 0))
            .with_string_to_value(s2v_f32_scale(1000.0, " ms".to_owned())),

            note: IntParam::new(
                "Note",
                // A4
                69,
                // C1..C9
                IntRange::Linear {
                    min: 60 - 12 * 2,
                    max: 60 + 12 * 2,
                },
            )
            .with_value_to_string(formatters::v2s_i32_note_formatter())
            .with_string_to_value(formatters::s2v_i32_note_formatter()),

            note_state: Default::default(),
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
                            ui.label("Gain");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                            ui.end_row();

                            ui.label("Attack");
                            ui.add(widgets::ParamSlider::for_param(&params.attack, setter));
                            ui.end_row();

                            ui.label("Release");
                            ui.add(widgets::ParamSlider::for_param(&params.release, setter));
                            ui.end_row();

                            ui.label("Note");
                            ui.add(widgets::ParamSlider::for_param(&params.note, setter));
                            ui.end_row();
                        });

                    let is_on = params.note_state.get() == NOTE_STATE_ON;
                    let button_clicked = ui.button(if is_on { "Pause" } else { "Play" }).clicked();
                    let key_pressed = ui
                        .input_mut()
                        .consume_key(egui::Modifiers::NONE, egui::Key::Space);
                    if button_clicked || key_pressed {
                        params.note_state.enqueue(!is_on);
                    }
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
        // handle note state sent from UI
        match self.params.note_state.dequeue() {
            Some(true) => {
                self.envelop.stage = EnvelopeStage::Attack(0.0);
            }
            Some(false) => {
                self.envelop.stage = EnvelopeStage::Release(0.0);
            }
            _ => {}
        }

        // sync params
        let note: u8 = self.params.note.value().try_into().unwrap();
        self.oscillator.frequency = nih_plug::util::midi_note_to_freq(note);
        self.envelop.attack_duration = self.params.attack.value();
        self.envelop.release_duration = self.params.release.value();

        // synthesize
        let sample_rate = context.transport().sample_rate;
        let duration_delta = sample_rate.recip();

        for samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            let envelope = self.envelop.next(duration_delta);
            let sine = self.oscillator.next(duration_delta);
            let value = gain * envelope * sine;
            for sample in samples {
                *sample = value;
            }
        }

        ProcessStatus::Normal
    }
}

//
// Oscillator
//

#[derive(Debug)]
struct Oscillator {
    // TODO: sine, square, triangle, sawtooth
    phase: f32,
    frequency: f32,
}

impl Oscillator {
    fn new() -> Self {
        Self {
            phase: 0.0,
            frequency: 440.0,
        }
    }

    fn next(&mut self, delta: f32) -> f32 {
        let value = (TAU * self.phase).sin();
        self.phase += self.frequency * delta;
        self.phase %= 1.0;
        value
    }
}

//
// Envelope
//

#[derive(Debug, Copy, Clone)]
enum EnvelopeStage {
    Off,
    Attack(f32),
    Sustain,
    Release(f32),
}

#[derive(Debug)]
struct Envelope {
    stage: EnvelopeStage,
    attack_duration: f32,
    release_duration: f32,
}

impl Envelope {
    fn new() -> Self {
        Self {
            stage: EnvelopeStage::Off,
            attack_duration: 0.01,
            release_duration: 0.1,
        }
    }

    fn next(&mut self, delta: f32) -> f32 {
        match self.stage {
            EnvelopeStage::Off => 0.0,
            EnvelopeStage::Attack(mut t) => {
                let value = t / self.attack_duration;
                t += delta;
                if t < self.attack_duration {
                    self.stage = EnvelopeStage::Attack(t);
                } else {
                    self.stage = EnvelopeStage::Sustain;
                }
                value
            }
            EnvelopeStage::Sustain => 1.0,
            EnvelopeStage::Release(mut t) => {
                let value = 1.0 - t / self.release_duration;
                t += delta;
                if t < self.release_duration {
                    self.stage = EnvelopeStage::Release(t);
                } else {
                    self.stage = EnvelopeStage::Off;
                }
                value
            }
        }
    }
}

//
// NoteState (copied from examples/midi_keyboard/src/lib.rs)
//

#[derive(Default)]
struct NoteState(AtomicIsize);

const NOTE_STATE_OFF: isize = 0;
const NOTE_STATE_ON_QUEUED: isize = 1;
const NOTE_STATE_ON: isize = 2;
const NOTE_STATE_OFF_QUEUED: isize = 3;

impl NoteState {
    fn set(&self, value: isize) {
        self.0.store(value, Ordering::Release);
    }

    fn get(&self) -> isize {
        self.0.load(Ordering::Acquire)
    }

    fn enqueue(&self, active: bool) {
        match (self.get(), active) {
            (NOTE_STATE_OFF, true) => {
                self.set(NOTE_STATE_ON_QUEUED);
            }
            (NOTE_STATE_ON, false) => {
                self.set(NOTE_STATE_OFF_QUEUED);
            }
            _ => {}
        }
    }

    fn dequeue(&self) -> Option<bool> {
        match self.get() {
            NOTE_STATE_ON_QUEUED => {
                self.set(NOTE_STATE_ON);
                Some(true)
            }
            NOTE_STATE_OFF_QUEUED => {
                self.set(NOTE_STATE_OFF);
                Some(false)
            }
            _ => None,
        }
    }
}

//
// millisecond formatter
//

fn v2s_f32_scale(scale: f32, digits: usize) -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(move |value| format!("{:.digits$}", value * scale))
}

fn s2v_f32_scale(scale: f32, trim_end: String) -> Arc<dyn Fn(&str) -> Option<f32> + Send + Sync> {
    Arc::new(move |string| {
        let string = string.trim_end_matches(&trim_end);
        string.parse::<f32>().ok().map(|value| value / scale)
    })
}
