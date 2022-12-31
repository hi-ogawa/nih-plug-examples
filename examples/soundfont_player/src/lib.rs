use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, egui, widgets, EguiState};
use std::{
    ffi::OsString,
    sync::{Arc, Mutex},
};

pub struct MyPlugin {
    params: Arc<MyParams>,
    synth: Arc<Mutex<oxisynth::Synth>>,
}

// embed 1KB of simple soundfont as default fallback
const DEFAULT_SOUNDFONT_BYTES: &[u8] =
    include_bytes!("../../../thirdparty/OxiSynth/testdata/sin.sf2");

lazy_static::lazy_static! {
    static ref DEFAULT_SOUNDFONT: oxisynth::SoundFont = {
        let mut cursor = std::io::Cursor::new(DEFAULT_SOUNDFONT_BYTES);
        let soundfont = oxisynth::SoundFont::load(&mut cursor).unwrap();
        soundfont
    };
}

#[derive(Params)]
struct MyParams {
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,

    #[id = "gain"]
    gain: FloatParam,

    // keep soundfont related states independently from `Synth` only for the used on gui thread
    // TODO: persist?
    // TODO: Arc<Mutex<...>> looks too verbose when we know these are only accessed on main thread
    soundfonts: Arc<Mutex<Vec<(String, OsString, oxisynth::SoundFont)>>>,
    soundfont: Arc<Mutex<Option<(String, OsString, oxisynth::SoundFont)>>>,
    bank: Arc<Mutex<Option<u32>>>,
    patch: Arc<Mutex<Option<(String, u32)>>>,
}

impl Default for MyPlugin {
    fn default() -> Self {
        let mut synth = oxisynth::Synth::default();
        synth.add_font(DEFAULT_SOUNDFONT.clone(), true);
        Self {
            params: Arc::new(MyParams::default()),
            synth: Arc::new(Mutex::new(synth)),
        }
    }
}

impl Default for MyParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(450, 300),

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

            soundfonts: Arc::new(Mutex::new(vec![])),
            soundfont: Arc::new(Mutex::new(None)),
            bank: Arc::new(Mutex::new(None)),
            patch: Arc::new(Mutex::new(None)),
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
        let mut synth = self.synth.lock().unwrap();
        synth.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn editor(&self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let synth = self.synth.clone();
        create_egui_editor(
            params.editor_state.clone(),
            (),
            |_, _| {},
            move |egui_ctx, setter, _state| {
                // TODO: more settings? (reverb, chorus)
                egui::CentralPanel::default().show(egui_ctx, |ui| {
                    egui::Grid::new("params")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("Gain");
                            ui.add(widgets::ParamSlider::for_param(&params.gain, setter));
                            ui.end_row();

                            //
                            // soundfont/bank/patch selector
                            //
                            let mut reset_synth = false;
                            let mut soundfonts = params.soundfonts.lock().unwrap();
                            let mut current_soundfont = params.soundfont.lock().unwrap();
                            let mut current_bank = params.bank.lock().unwrap();
                            let mut current_patch = params.patch.lock().unwrap();
                            let mut bank_options: Vec<u32> = vec![];
                            let mut patch_options: Vec<(String, u32)> = vec![];
                            if let Some((_, _, soundfont)) = &*current_soundfont {
                                for preset in &soundfont.presets {
                                    bank_options.push(preset.banknum());
                                    if *current_bank == Some(preset.banknum()) {
                                        patch_options
                                            .push((preset.name().to_string(), preset.num()));
                                    }
                                }
                            }
                            bank_options.sort();
                            bank_options.dedup();

                            ui.label("Soundfont");
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_source("soundfont")
                                    .width(200.0)
                                    .selected_text(current_soundfont.as_ref().map_or("", |v| &v.0))
                                    .show_ui(ui, |ui| {
                                        for el in soundfonts.iter() {
                                            let selected = current_soundfont
                                                .as_ref()
                                                .map_or(false, |v| v.0 == el.0);
                                            let mut response =
                                                ui.selectable_label(selected, el.0.clone());
                                            if response.clicked() {
                                                *current_soundfont = Some(el.clone());
                                                *current_bank = None;
                                                *current_patch = None;
                                                reset_synth = true;
                                                response.mark_changed();
                                            }
                                        }
                                    });

                                //
                                // file dialog to choose soundfont https://github.com/emilk/egui/blob/34f587d1e1cc69146f7a02f20903e4f573030ffd/examples/file_dialog/src/main.rs
                                //
                                if ui.button("Load File").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                                        // TODO: error handling
                                        // TODO: do asynchronous since it takes a few seconds to read a large file?
                                        let mut file = std::fs::File::open(path.clone()).unwrap();
                                        let soundfont =
                                            oxisynth::SoundFont::load(&mut file).unwrap(); // TODO: does it hang when loading invalid files?
                                        let file_name =
                                            path.file_name().unwrap().to_string_lossy().to_string();
                                        let path_string = path.as_os_str().to_os_string();
                                        soundfonts.push((file_name, path_string, soundfont));
                                    }
                                }
                            });
                            ui.end_row();

                            // TODO: probably table ui is better
                            ui.label("Bank / Patch");
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_source("bank")
                                    .width(50.0)
                                    .selected_text(
                                        current_bank.map_or("".to_string(), |v| v.to_string()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for &bank in &bank_options {
                                            let mut response = ui.selectable_label(
                                                *current_bank == Some(bank),
                                                bank.to_string(),
                                            );
                                            if response.clicked() {
                                                *current_bank = Some(bank);
                                                *current_patch = None;
                                                reset_synth = true;
                                                response.mark_changed();
                                            }
                                        }
                                    });

                                egui::ComboBox::from_id_source("patch")
                                    .width(250.0)
                                    .selected_text(
                                        current_patch
                                            .as_ref()
                                            .map_or("".to_string(), |v| v.0.clone()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (name, patch) in &patch_options {
                                            let selected = current_patch
                                                .as_ref()
                                                .map_or(false, |v| &v.1 == patch);
                                            let mut response = ui.selectable_label(selected, name);
                                            if response.clicked() {
                                                *current_patch = Some((name.clone(), *patch));
                                                reset_synth = true;
                                                response.mark_changed();
                                            }
                                        }
                                    });
                            });
                            ui.end_row();

                            if reset_synth {
                                let mut synth = synth.lock().unwrap();
                                // remove current font
                                synth.font_bank_mut().reset();

                                // select preset or fallback
                                if current_soundfont.is_some()
                                    && current_bank.is_some()
                                    && current_patch.is_some()
                                {
                                    let font_id = synth.add_font(
                                        current_soundfont.as_ref().unwrap().2.clone(),
                                        true,
                                    );
                                    synth
                                        .program_select(
                                            0, // TODO: hard-code channel?
                                            font_id,
                                            current_bank.unwrap(),
                                            current_patch.as_ref().unwrap().1.try_into().unwrap(),
                                        )
                                        .unwrap();
                                } else {
                                    synth.add_font(DEFAULT_SOUNDFONT.clone(), true);
                                }
                            }
                        });
                });
            },
        )
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // allow main thread to lock `Synth` when changing soundfont/preset
        match self.synth.try_lock().as_mut() {
            Ok(synth) => self.process_inner(buffer, aux, context, synth),
            _ => ProcessStatus::KeepAlive,
        }
    }
}

impl MyPlugin {
    fn process_inner(
        &self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
        synth: &mut oxisynth::Synth,
    ) -> ProcessStatus {
        //
        // handle note on/off
        //

        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn {
                    timing: _, // TODO: timing offset
                    voice_id: _,
                    channel: _,
                    note,
                    velocity,
                } => {
                    synth
                        .send_event(oxisynth::MidiEvent::NoteOn {
                            channel: 0,
                            key: note,
                            vel: denormalize_velocity(velocity) as u8,
                        })
                        .unwrap();
                }
                NoteEvent::NoteOff {
                    timing: _,
                    voice_id: _,
                    channel: _,
                    note,
                    velocity: _,
                } => {
                    synth
                        .send_event(oxisynth::MidiEvent::NoteOff {
                            channel: 0,
                            key: note,
                        })
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
