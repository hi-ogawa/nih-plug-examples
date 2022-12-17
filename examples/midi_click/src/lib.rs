use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::sync::Arc;

pub struct MyPlugin {
    params: Arc<MyParams>,
    sample_offset: usize,
}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "bpm"]
    bpm: IntParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            sample_offset: 0,
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(200, 80),
            bpm: IntParam::new("BPM", 150, IntRange::Linear { min: 1, max: 300 }),
        }
    }
}

impl Plugin for MyPlugin {
    // https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates
    const NAME: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const VENDOR: &'static str = "xxx";
    const URL: &'static str = "xxx";
    const EMAIL: &'static str = "xxx";

    // IO ports
    const DEFAULT_INPUT_CHANNELS: u32 = 0;
    const DEFAULT_OUTPUT_CHANNELS: u32 = 1; // TODO: cannot know the process buffer size without output port?
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;

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
                    ui.label("BPM");
                    ui.add(widgets::ParamSlider::for_param(&params.bpm, setter));
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
        let bpm = self.params.bpm.smoothed.next() as f32;
        let click_sample_interval = (sample_rate / (bpm / 60.0)) as usize;

        for buffer_offset in 0..buffer.len() {
            self.sample_offset %= click_sample_interval;
            if self.sample_offset == 0 {
                // TODO: should send NoteOff?
                context.send_event(NoteEvent::NoteOn {
                    timing: buffer_offset as u32,
                    voice_id: None,
                    channel: 0,
                    note: 69,
                    velocity: 0.5,
                });
            }
            self.sample_offset += 1;
        }

        ProcessStatus::Normal
    }
}
