use nih_plug::prelude::nih_export_standalone;
use soundfont_player::MyPlugin;

fn main() {
    nih_export_standalone::<MyPlugin>();
}
