use nih_plug::prelude::nih_export_standalone;
use simple_synth::MyPlugin;

fn main() {
    nih_export_standalone::<MyPlugin>();
}
