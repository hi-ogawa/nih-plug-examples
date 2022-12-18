use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets::ParamSlider, EguiState};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc,
    },
};

pub struct MyPlugin {
    params: Arc<MyParams>,
    note_states: Vec<Arc<NoteState>>,
}

#[derive(Params)]
pub struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "channel"]
    channel: IntParam,

    #[id = "velocity"]
    velocity: FloatParam,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            params: Arc::new(MyParams::default()),
            note_states: (0..128).map(|_| Arc::new(NoteState::default())).collect(),
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(500, 160), // TODO: adapt to window resize? (https://github.com/RustAudio/baseview/pull/136)
            channel: IntParam::new("channel", 0, IntRange::Linear { min: 0, max: 15 }),
            velocity: FloatParam::new("velocity", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 }),
        }
    }
}

impl Plugin for MyPlugin {
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
        let channel = self.params.channel.value() as u8;
        let velocity = self.params.velocity.value();

        // iterate all notes
        for (note, note_state) in self.note_states.iter().enumerate() {
            match note_state.dequeue() {
                Some(true) => {
                    context.send_event(NoteEvent::NoteOn {
                        timing: 0,
                        voice_id: None,
                        channel,
                        note: note as u8,
                        velocity,
                    });
                }
                Some(false) => {
                    context.send_event(NoteEvent::NoteOff {
                        timing: 0,
                        voice_id: None,
                        channel,
                        note: note as u8,
                        velocity,
                    });
                }
                _ => {}
            };
        }
        ProcessStatus::Normal
    }

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let note_states = self.note_states.clone();
        let is_initial_render = AtomicBool::new(true); // editor callback should be on the same thread, but needs atomic to pass borrow check
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    egui::Grid::new("params")
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Channel");
                            ui.add(ParamSlider::for_param(&params.channel, setter));
                            ui.end_row();

                            ui.label("Velocity");
                            ui.add(ParamSlider::for_param(&params.velocity, setter));
                            ui.end_row();
                        });

                    ui.separator();

                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        let (response, active_notes) = piano_ui(ui);
                        for (note, note_state) in note_states.iter().enumerate() {
                            note_state.enqueue(active_notes.contains(&(note as u8)));
                        }
                        // scroll to center on initial render
                        if is_initial_render.load(Ordering::Relaxed) {
                            is_initial_render.store(false, Ordering::Relaxed);
                            ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
                        }
                    });
                });
            },
        )
    }
}

//
// ui
//

#[derive(Debug, Clone, Copy)]
struct NoteRect {
    note: u8,
    rect: egui::Rect,
}

pub fn piano_ui(ui: &mut egui::Ui) -> (egui::Response, HashSet<u8>) {
    const C4: u8 = 60;
    const OCTAVE: u8 = 12;
    let note_rects = generate_note_rects(C4 - 3 * OCTAVE, C4 + 3 * OCTAVE);

    // allocate geometry
    let paint_rect = note_rects
        .iter()
        .fold(egui::Rect::NOTHING, |acc, el| acc.union(el.rect));
    let (mut response, painter) =
        ui.allocate_painter(paint_rect.size(), egui::Sense::click_and_drag());

    //
    // handle UI event
    //
    let mut active_note_by_pointer: Option<u8> = None; // (note that black key rect overlaps with white ones)
    if let Some(pointer_pos) = response.interact_pointer_pos() {
        let local_pos = pointer_pos - response.rect.min.to_vec2();
        for &el in &note_rects {
            if el.rect.contains(local_pos) {
                active_note_by_pointer = Some(el.note);
            }
        }
    }

    //
    // keyboard shortcut
    //
    let mut active_notes: HashSet<u8> = HashSet::new();
    if let Some(note) = active_note_by_pointer {
        active_notes.insert(note);
    }

    let note_to_key = {
        use egui::Key::*;
        // "zsxdcvgbhnjm".split("").map(c => c.toUpperCase()).join(", ")
        // "q2w3er5t6y7ui9o0p".split("").map(c => Number.isInteger(Number(c)) ? `Num${c}` : c.toUpperCase()).join(", ")
        let keys1 = [Z, S, X, D, C, V, G, B, H, N, J, M];
        let keys2 = [
            Q, Num2, W, Num3, E, R, Num5, T, Num6, Y, Num7, U, I, Num9, O, Num0, P,
        ];
        let zip1 = ((C4 - OCTAVE)..).zip(keys1);
        let zip2 = (C4..).zip(keys2);
        zip1.chain(zip2)
    };

    for (note, key) in note_to_key {
        if ui.ctx().input().key_down(key) {
            active_notes.insert(note);
        }
    }

    //
    // render
    //
    for &el in &note_rects {
        let rect = el.rect.translate(response.rect.min.to_vec2());
        let mut color = if is_black_key(el.note as usize) {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        };
        if active_notes.contains(&el.note) {
            response.mark_changed();
            color = egui::Color32::LIGHT_BLUE;
        }
        painter.rect_filled(rect, egui::Rounding::from(1.0), color);

        // put "note label" on top of key (e.g. C4)
        if el.note % 12 == 0 {
            painter.text(
                rect.left_bottom() + egui::vec2(2.0, -2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("C{}", (el.note / 12) - 1),
                egui::FontId::monospace(14.0),
                egui::Color32::BLACK,
            );
        }
    }

    (response, active_notes)
}

fn is_black_key(note: usize) -> bool {
    match note % 12 {
        1 | 3 | 6 | 8 | 10 => true,
        _ => false,
    }
}

fn key_offset(note: usize) -> usize {
    (note / 12) * 14
        + match note % 12 {
            x if x >= 5 => x + 1,
            x => x,
        }
}

fn generate_note_rects(note_begin: u8, note_end: u8) -> Vec<NoteRect> {
    const PADDING: f32 = 1.0;
    const KEY_SIZE: egui::Vec2 = egui::Vec2::new(20.0, 80.0);

    let mut result = vec![];

    for note in note_begin..note_end {
        let x_offset = key_offset(note as usize) - key_offset(note_begin as usize);
        let pos = egui::pos2(0.5 * (x_offset as f32) * (KEY_SIZE.x + 2.0 * PADDING), 0.0);
        let size = if is_black_key(note as usize) {
            KEY_SIZE * egui::vec2(1.0, 0.5)
        } else {
            KEY_SIZE
        };
        let rect = egui::Rect::from_min_size(pos, size);
        result.push(NoteRect { note, rect });
    }

    result.sort_by_key(|el| is_black_key(el.note as usize)); // black key has higher z
    result
}

//
// inter-thread note state management
//

// flowchart in mermaid https://mermaid.live/edit#pako:eNptj11rwjAUhv9KOFcb1NLUdm1zMVCrMAa6Ib2ZKSOYdBZsIlmK68T_vlPFfWEuDsk575MnOcDaSAUMqq3ZrzfCOq4JrtFqMZuVZDAgHB5VR3Kz1-SmeCBuY5WQtxxwdk_Gq8X89bmYFtO8PINjMvCRWSotydw4tUBs1Mra_CJ9JCdIlj1wxiY_quLpiijv3_PXlP83VdV11Qg8aJRtRC3xo4ee5uA2qlEcGG6lqkS7dRy4PmK03Unh1FTWzlhgldi-Kw9E68yy02tgzrbqEspr8WZF853aCf1iTHMJ4RHYAT6AhdmdT9OMJsMwS-IoSjzogNE09CkNsDFMcBIl8dGDz9MFgR9HAQJ9CYMwTenxCya1e88
// flowchart
//     0[OFF] -- "key down (UI thread)" --> 1[ON_QUEUED]
//     1 -. "send NoteOn (Audio thread)" .-> 2[ON]
//     2 -- "key up (UI thread)" --> 3[OFF_QUEUED]
//     3 -. "send NoteOff (Audio thread)" .-> 0
const NOTE_STATE_OFF: isize = 0;
const NOTE_STATE_ON_QUEUED: isize = 1;
const NOTE_STATE_ON: isize = 2;
const NOTE_STATE_OFF_QUEUED: isize = 3;

#[derive(Debug, Default)]
struct NoteState(AtomicIsize);

impl NoteState {
    fn set(&self, value: isize) {
        self.0.store(value, Ordering::Relaxed);
    }

    fn get(&self) -> isize {
        self.0.load(Ordering::Relaxed)
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
