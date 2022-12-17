use nih_plug::prelude::nih_export_standalone;
use ui_to_midi::MyPlugin;

fn main() {
    nih_export_standalone::<MyPlugin>();
}
