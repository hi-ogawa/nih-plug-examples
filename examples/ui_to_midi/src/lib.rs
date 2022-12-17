use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::sync::{atomic::AtomicIsize, Arc};

pub struct MyPlugin {
    params: Arc<MyParams>,
    // state diagram in mermaid https://mermaid.live/edit#pako:eNplj7EKwjAQhl8l3KTQgm23DIKgg4sO4pYl9q620CYlvShS-u6mFdTqTcfPdx_395BbJJDQsWbaVvrqdBPfUmVEmJWI47VIpLh4ZmsE2nvIF-e94NKRxuULSyYslaIjg-JgmY4jtvFY2TmZTmT2Fvp2TP-E2YStvoVF8SOECBpyja4wfN-Pdwq4pIYUyLAiFdrXrECZIaDasz09TA6SnacIfIufvvNwhxVbB7LQdUfDE8atWfI
    //   stateDiagram-v2
    //   0 --> 1: button down  (UI thread)
    //   1 --> 2: send NoteOn  (Audio thread)
    //   2 --> 3: button up    (UI thread)
    //   3 --> 0: send NoteOff (Audio thread)
    play_state: Arc<AtomicIsize>,
}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "note"]
    pub note: IntParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            play_state: Arc::new(AtomicIsize::new(0)),
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(300, 180),
            note: IntParam::new(
                "Note",
                // A4
                69,
                // C4..C6
                IntRange::Linear {
                    min: 60,
                    max: 60 + 12 * 2,
                },
            )
            .with_value_to_string(formatters::v2s_i32_note_formatter())
            .with_string_to_value(formatters::s2v_i32_note_formatter()),
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

    const DEFAULT_INPUT_CHANNELS: u32 = 0;
    const DEFAULT_OUTPUT_CHANNELS: u32 = 0;
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::MidiCCs;

    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        _buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // TODO: potentially `note` used for NoteOn and NoteOff will be different
        let note = self.params.note.value() as u8;
        let current_state = self.play_state.load(std::sync::atomic::Ordering::Relaxed);
        match current_state {
            1 => {
                context.send_event(NoteEvent::NoteOn {
                    timing: 0,
                    voice_id: None,
                    channel: 0,
                    note,
                    velocity: 0.5,
                });
                self.play_state
                    .store(2, std::sync::atomic::Ordering::Relaxed);
            }
            3 => {
                context.send_event(NoteEvent::NoteOff {
                    timing: 0,
                    voice_id: None,
                    channel: 0,
                    note,
                    velocity: 0.5,
                });
                self.play_state
                    .store(0, std::sync::atomic::Ordering::Relaxed);
            }
            _ => {}
        }
        ProcessStatus::Normal
    }

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let play_state = self.play_state.clone();
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    // "Note" slider input
                    ui.label("Note");
                    ui.add(widgets::ParamSlider::for_param(&params.note, setter));

                    // "Play" button/key
                    let button_state = ui.button("Play").is_pointer_button_down_on()
                        || egui_ctx.input_mut().key_down(egui::Key::Space);
                    let current_state = play_state.load(std::sync::atomic::Ordering::Relaxed);
                    match (button_state, current_state) {
                        (true, 0) => {
                            play_state.store(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        (false, 2) => {
                            play_state.store(3, std::sync::atomic::Ordering::Relaxed);
                        }
                        _ => {}
                    }
                });
            },
        )
    }
}
