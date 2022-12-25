use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets::ParamSlider, EguiState};
use std::sync::{Arc, Mutex};

// note that `Mutex.try_lock` is used instead of `Mutex.lock` in all places
// since the usages of `Arc<Mutex<...>>` are only for borrow-checker workaround and
// each case is pre-determined to be accssed by a single thread.
// the necessity of workaround might be due to typing of `create_egui_editor` which requires `Sync` for callbacks.

pub struct MyPlugin {
    params: Arc<MyParams>,

    // audio thread owns `Plugin` via `process(&mut Plugin)` so other threads (gui/background) requires `Arc<Mutex<...>>` to workaround mutability.
    // note that when node is "popped" in audio thread, it has to be moved back to a queue to avoid drop/deallocation.
    note_queue_producer: Arc<Mutex<llq::Producer<MyEvent>>>, // gui
    note_queue_consumer: llq::Consumer<MyEvent>,             // audio
    note_queue_producer_drop: llq::Producer<MyEvent>,        // audio
    note_queue_consumer_drop: Arc<Mutex<llq::Consumer<MyEvent>>>, // gui

    note_state: Arc<Mutex<Vec<bool>>>, // gui
}

struct MyEvent(u8, bool); // (note, on/off)

#[derive(Params)]
pub struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "channel"]
    channel: IntParam,

    #[id = "velocity"]
    velocity: FloatParam,
}

const MAX_NOTE: usize = 128;

impl Default for MyPlugin {
    fn default() -> Self {
        let (tx1, rx1) = llq::Queue::new().split();
        let (tx2, rx2) = llq::Queue::new().split();
        Self {
            params: Arc::new(MyParams::default()),
            note_queue_producer: Arc::new(Mutex::new(tx1)),
            note_queue_consumer: rx1,
            note_queue_producer_drop: tx2,
            note_queue_consumer_drop: Arc::new(Mutex::new(rx2)),
            note_state: Arc::new(Mutex::new(vec![false; MAX_NOTE])),
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

        while let Some(node) = self.note_queue_consumer.pop() {
            let MyEvent(note, is_on) = *node;
            if is_on {
                context.send_event(NoteEvent::NoteOn {
                    timing: 0,
                    voice_id: None,
                    channel,
                    note,
                    velocity,
                });
            } else {
                context.send_event(NoteEvent::NoteOff {
                    timing: 0,
                    voice_id: None,
                    channel,
                    note,
                    velocity,
                });
            }
            self.note_queue_producer_drop.push(node); // need to move `node` back to somewhere to avoid dropping on audio thread
        }
        ProcessStatus::Normal
    }

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let note_queue_producer = self.note_queue_producer.clone();
        let note_queue_consumer_drop = self.note_queue_consumer_drop.clone();
        let note_states = self.note_state.clone();
        let is_initial_render = Arc::new(Mutex::new(true));
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _| {
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
                        let (response, note_states_ui) = piano_ui(ui);

                        let mut note_states = note_states.try_lock().unwrap();
                        let mut note_queue_producer = note_queue_producer.try_lock().unwrap();
                        for note in 0..MAX_NOTE {
                            match (note_states[note], note_states_ui[note]) {
                                (false, true) => {
                                    note_states[note] = true;
                                    note_queue_producer
                                        .push(llq::Node::new(MyEvent(note as u8, true)));
                                }
                                (true, false) => {
                                    note_states[note] = false;
                                    note_queue_producer
                                        .push(llq::Node::new(MyEvent(note as u8, false)));
                                }
                                _ => {}
                            }
                        }
                        // scroll to center on initial render
                        let mut is_initial_render = is_initial_render.try_lock().unwrap();
                        if *is_initial_render {
                            *is_initial_render = false;
                            ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
                        }
                    });

                    // cleanup llq nodes
                    let mut note_queue_consumer_drop = note_queue_consumer_drop.try_lock().unwrap();
                    while let Some(_node_to_drop) = note_queue_consumer_drop.pop() {}
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

pub fn piano_ui(ui: &mut egui::Ui) -> (egui::Response, Vec<bool>) {
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
    let mut note_states: Vec<bool> = vec![false; MAX_NOTE];
    if let Some(note) = active_note_by_pointer {
        note_states[note as usize] = true;
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
            note_states[note as usize] = true;
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
        if note_states[el.note as usize] {
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

    (response, note_states)
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
